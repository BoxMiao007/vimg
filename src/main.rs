mod command;
mod process;
mod temporary;

use std::error::Error;

use clap::error::{ContextKind, ContextValue, ErrorFormatter, ErrorKind};
use clap::{CommandFactory, FromArgMatches, Parser};

/// 从视频抽出截帧，排成接触表，再编码成 AVIF。
///
/// 一条命令做完抽帧、拼网格、编码。输出可以是静图，也可以是多帧动画。
/// 运行时必须能在 PATH 里找到 ffmpeg 和 ffprobe。
///
/// 不写子命令时打印这份说明并以状态码 2 退出。
#[derive(Parser)]
#[command(
    version,
    about,
    long_about = None,
    styles = clap::builder::Styles::plain(),
    disable_help_subcommand = true,
    override_usage = "vimg <子命令>",
    after_help = "示例:\n  \
        vimg vcs -c 5 -n 25 -H 288 视频.mkv\n  \
        vimg vcs --layout 1 -n 2 视频.mkv\n  \
        vimg extract -n 12 视频.mkv\n  \
        vimg join -c 4 -o sheet.png 帧1.bmp 帧2.bmp\n\n\
        某个子命令的全部选项: vimg <子命令> --help"
)]
enum Command {
    Vcs(command::Vcs),
    Join(command::Join),
    Extract(command::Extract),
    PrintCompletions(command::PrintCompletions),
}

impl Command {
    /// clap 写死的英文标题换成中文。子命令自己的说明仍来自各自的文档注释。
    fn command() -> clap::Command {
        let mut cmd = localize(<Self as CommandFactory>::command());
        // `--help`、`--version` 要等 build 才会出现，而且 mut_args 改不到它们。
        cmd.build();
        translate_builtin(&mut cmd);
        cmd
    }
}

fn localize(mut cmd: clap::Command) -> clap::Command {
    const HELP: &str = "{about-with-newline}\
        用法: {usage}\n\
        \n\
        {all-args}{after-help}";
    cmd = cmd
        .help_template(HELP)
        .subcommand_help_heading("子命令")
        .mut_args(|arg| {
            let heading = if arg.is_positional() {
                "参数"
            } else {
                "选项"
            };
            arg.help_heading(heading)
        });
    cmd.mut_subcommands(localize)
}

fn translate_builtin(cmd: &mut clap::Command) {
    *cmd = std::mem::take(cmd).mut_args(|arg| match arg.get_id().as_str() {
        // 内建开关不吃 next_help_heading，不归组就会单独冒出一个英文 Options。
        "help" => arg
            .help("打印这份说明")
            .long_help("打印这份说明")
            .help_heading("选项"),
        "version" => arg
            .help("打印版本号")
            .long_help("打印版本号")
            .help_heading("选项"),
        _ => arg,
    });
    let names: Vec<String> = cmd
        .get_subcommands()
        .map(|sub| sub.get_name().to_string())
        .collect();
    for name in names {
        *cmd = std::mem::take(cmd).mut_subcommand(name, |mut sub| {
            translate_builtin(&mut sub);
            sub
        });
    }
}

fn main() -> anyhow::Result<()> {
    let mut built = Command::command();
    let matches = match built.try_get_matches_from_mut(std::env::args_os()) {
        Ok(matches) => matches,
        Err(err) => {
            // format 只补用法，不会换成自定义格式器，所以这里自己打印。
            err.apply::<ZhError>().format(&mut built).exit();
        }
    };
    let cmd = Command::from_arg_matches(&matches)?;

    _ = ctrlc::set_handler(|| {
        temporary::clean();
        std::process::exit(1);
    });

    let result = run(cmd);

    temporary::clean();

    result
}

/// clap 的报错句子是英文。按它带来的种类和上下文重写成中文，用法行仍用原来的。
struct ZhError;

impl ErrorFormatter for ZhError {
    fn format_error(error: &clap::error::Error<Self>) -> clap::builder::StyledStr {
        let mut text = String::from("错误: ");
        if !write_reason(&mut text, error) {
            text.push_str(error.kind().as_str().unwrap_or("未知错误"));
        }
        if let Some(ContextValue::String(name)) = error.get(ContextKind::SuggestedArg) {
            text.push_str(&format!("\n  提示: 是不是要写 '{name}'"));
        }
        if let Some(ContextValue::String(name)) = error.get(ContextKind::SuggestedSubcommand) {
            text.push_str(&format!("\n  提示: 是不是要写 '{name}'"));
        }
        if let Some(ContextValue::String(name)) = error.get(ContextKind::SuggestedValue) {
            text.push_str(&format!("\n  提示: 是不是要写 '{name}'"));
        }
        if let Some(ContextValue::StyledStr(usage)) = error.get(ContextKind::Usage) {
            let usage = usage.to_string().replacen("Usage:", "用法:", 1);
            text.push_str("\n\n");
            text.push_str(usage.trim_end());
        }
        text.push_str("\n\n详见 --help。\n");
        text.into()
    }
}

fn write_reason(text: &mut String, error: &clap::error::Error<ZhError>) -> bool {
    let arg = string(error, ContextKind::InvalidArg);
    let value = string(error, ContextKind::InvalidValue);
    match error.kind() {
        ErrorKind::MissingRequiredArgument => {
            let Some(ContextValue::Strings(missing)) = error.get(ContextKind::InvalidArg) else {
                return false;
            };
            text.push_str("缺少必填参数:");
            for item in missing {
                text.push_str("\n  ");
                text.push_str(item);
            }
        }
        ErrorKind::UnknownArgument => {
            let Some(name) = arg else { return false };
            text.push_str(&format!("不认识的参数 '{name}'"));
        }
        ErrorKind::InvalidSubcommand => {
            let Some(name) = string(error, ContextKind::InvalidSubcommand) else {
                return false;
            };
            text.push_str(&format!("不认识的子命令 '{name}'"));
            list(text, "可用子命令", error.get(ContextKind::ValidSubcommand));
        }
        ErrorKind::MissingSubcommand => {
            text.push_str("需要一个子命令");
            list(text, "可用子命令", error.get(ContextKind::ValidSubcommand));
        }
        ErrorKind::InvalidValue => match (arg, value) {
            (Some(name), Some(given)) if given.is_empty() => {
                text.push_str(&format!("'{name}' 需要一个值"));
            }
            (Some(name), Some(given)) => {
                text.push_str(&format!("'{name}' 的值 '{given}' 无效"));
                list(text, "可选值", error.get(ContextKind::ValidValue));
            }
            _ => return false,
        },
        ErrorKind::ValueValidation => {
            let (Some(name), Some(given)) = (arg, value) else {
                return false;
            };
            text.push_str(&format!("'{name}' 的值 '{given}' 无效"));
            if let Some(source) = error.source() {
                text.push_str(": ");
                text.push_str(&source.to_string());
            }
        }
        ErrorKind::ArgumentConflict => {
            let Some(name) = arg.or_else(|| string(error, ContextKind::InvalidSubcommand)) else {
                return false;
            };
            text.push_str(&format!("'{name}' 不能和以下参数一起用:"));
            match error.get(ContextKind::PriorArg) {
                Some(ContextValue::String(prior)) => {
                    text.push_str("\n  ");
                    text.push_str(prior);
                }
                Some(ContextValue::Strings(prior)) => {
                    for item in prior {
                        text.push_str("\n  ");
                        text.push_str(item);
                    }
                }
                _ => {}
            }
        }
        ErrorKind::TooManyValues | ErrorKind::TooFewValues | ErrorKind::WrongNumberOfValues => {
            let Some(name) = arg else { return false };
            text.push_str(&format!("'{name}' 的值个数不对"));
        }
        ErrorKind::InvalidUtf8 => text.push_str("参数不是有效的 UTF-8"),
        ErrorKind::DisplayHelp
        | ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
        | ErrorKind::DisplayVersion
        | ErrorKind::Io
        | ErrorKind::Format
        | ErrorKind::NoEquals => return false,
        _ => return false,
    }
    true
}

fn string(error: &clap::error::Error<ZhError>, kind: ContextKind) -> Option<String> {
    match error.get(kind) {
        Some(ContextValue::String(value)) => Some(value.clone()),
        _ => None,
    }
}

fn list(text: &mut String, title: &str, values: Option<&ContextValue>) {
    let Some(ContextValue::Strings(values)) = values else {
        return;
    };
    if values.is_empty() {
        return;
    }
    text.push_str("\n  [");
    text.push_str(title);
    text.push_str(": ");
    text.push_str(&values.join(", "));
    text.push(']');
}

fn run(cmd: Command) -> anyhow::Result<()> {
    match cmd {
        Command::Vcs(c) => c.run()?,
        Command::Join(c) => c.run()?,
        Command::Extract(c) => {
            let ex = c.run()?;
            for msg in ex.warnings {
                eprintln!("Warning: {msg}");
            }
        }
        Command::PrintCompletions(c) => c.run(),
    }

    Ok(())
}
