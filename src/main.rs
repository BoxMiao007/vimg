mod command;
mod process;
mod temporary;

use std::error::Error;

use clap::error::{ContextKind, ContextValue, ErrorFormatter, ErrorKind};
use clap::{CommandFactory, FromArgMatches, Parser};

/// `vimg -h` 在子命令列表后面写出每个命令的全部选项。
/// 正文放这里，避免属性字符串里的引号和换行把宏写乱。
const HELP_BODY: &str = "\
示例:
  下面几条是最常用的写法。尖括号里的名字换成你自己的文件。
  vimg vcs -c 5 -n 25 -H 288 视频.mkv
    等大网格：抽 25 格，每行 5 格，每格高 288 像素，编码成同名 AVIF。
  vimg vcs -l 1 -n 2 视频.mkv
    固定版式排两截。每截 14 格，中间另有一行小格，一共 32 张。
  vimg extract -n 12 视频.mkv
    只抽出 12 张 BMP，不拼图、不编码。
  vimg join -c 4 -o sheet.png 帧1.bmp 帧2.bmp
    把已有截帧按每行 4 格拼成 sheet.png。
  vimg print-completions fish
    打印 fish 的补全脚本。不写 shell 时是 bash。

命令与选项:

  vcs
    抽帧、拼网格、编码成 AVIF。等大网格必须给 -n、-c，以及 -H 或 -W。
    -l 1 是固定版式：一截 14 格，4 列、5 个行单位，左上和右下各一个 2×2 大格，
    正中间一行 4 个小格。这时 -n 是截数，默认 1；-c、-H、-W 被忽略并提示。
    输出扩展名必须是 .avif。不写 -o 时用输入文件名换扩展名。
    用法: vimg vcs [选项] <视频>
      -c, --columns <列数>       等大网格的列数。必填，除非 -l 1。
      -o, --output <文件>        输出的 AVIF。扩展名必须是 .avif。
      -W, --capture-width <像素> 每一格的宽度，按比例缩放。与 -H 取一。
      -H, --capture-height <像素>
                                 每一格的高度，按比例缩放。与 -W 取一。等大网格必填其一。
      -n, --number <数量>        等大网格是格数，必填。-l 1 时是截数，不写默认 1。
      -f, --capture-frames <帧数>
                                 每一格抽几帧。不写是 30。-f 1 是静图。
      -t, --capture-time <时长>  多帧时每一格取多长素材。默认 1500ms。
      -T, --threads <数量>       同时跑几路 ffmpeg。默认 3，写 0 则按 CPU 数自动决定。
      -l, --layout <编号>        接触表版式。不写是等大网格。目前只有 1。
          --avif-crf <质量>      编码质量，越小越清晰。默认 30。
          --avif-codec <编码器>  AVIF 编码器。默认 libsvtav1。
          --avif-preset <预设>   编码速度。不写时单帧用 1，多帧用 6。
          --avif-fps <帧率>      多帧 AVIF 的播放帧率。默认 20。静图不受影响。
          --ignore-start <时间>  采样时忽略开头。可写 30s、1500ms、5%。默认 0s。
          --ignore-end <时间>    采样时忽略结尾。写法同上。默认 0s。
          --vfilter <滤镜>       追加的 ffmpeg 视频滤镜，接在缩放后面。
          --output-dir <目录>    临时目录建在这个目录下。截帧写进临时目录。
          --keep                 退出时保留临时目录，并在开始时打印路径。
          --info                 画简要参数栏。这是默认行为，写不写一样。
          --info-all             画完整参数栏：其余视频轨和音轨、总帧数、容器。
          --no-info              不画参数栏。右下角采样时刻不受影响。
          --font <文件>          参数栏字体。不写则用环境变量 VIMG_FONT，再没有用内嵌 MiSans。
      <视频>                     要抽帧的视频文件。

  extract
    只抽 BMP，不拼图、不编码。-n 必填，-f 默认 1。
    文件写到当前目录或 --output-dir，文件名形如 视频名-12s-01.bmp。
    用法: vimg extract [选项] <视频>
      -n, --number <数量>        等距采样的点数。必填。
      -f, --capture-frames <帧数>
                                 每一格抽几帧。不写是 1，只出静帧。
      -t, --capture-time <时长>  多帧时每一格取多长素材。默认 1500ms。
      -T, --threads <数量>       同时跑几路 ffmpeg。默认 3，写 0 则按 CPU 数自动决定。
          --ignore-start <时间>  采样时忽略开头。可写 30s、1500ms、5%。默认 0s。
          --ignore-end <时间>    采样时忽略结尾。写法同上。默认 0s。
          --vfilter <滤镜>       原样传给 ffmpeg 的 -vf。
          --output-dir <目录>    截帧写到这个目录。不写是当前目录。目录不存在会创建。
      <视频>                     要抽帧的视频文件。

  join
    把已有截帧拼成一张图。不调用 ffmpeg，除非为了画参数栏而去探测 --video。
    等大网格必须给 -c。-l 1 的张数必须是 14、32、50……对不上就不出图。
    用法: vimg join [选项] -o <输出> <图片>...
      -c, --columns <列数>       等大网格的列数。必填，除非 -l 1。
      -W, --capture-width <像素> 每一格的宽度。与 -H 可同时写，也都可以不写。
      -H, --capture-height <像素>
                                 每一格的高度。都不写就用原图尺寸。
      -o, --output <文件>        输出图片。扩展名决定格式，常见 png、jpg、bmp。
      -l, --layout <编号>        接触表版式。不写是等大网格。目前只有 1。
          --label <文字>         印在格子右下角。可重复，按顺序对应每一张图。
          --video <视频>         用来画参数栏的源视频。不给就不画参数栏。
          --info                 画简要参数栏。必须同时给出 --video。
          --info-all             画完整参数栏。必须同时给出 --video。
          --no-info              不画参数栏。
          --font <文件>          参数栏字体。不写则用环境变量 VIMG_FONT，再没有用内嵌 MiSans。
      <图片>...                  要拼进去的图片，至少一张。等大网格要求尺寸相同。

  print-completions
    把 shell 补全脚本打到标准输出。不写 shell 时是 bash。
    用法: vimg print-completions [shell]
      [shell]                    bash、fish、zsh、powershell、elvish 之一。默认 bash。

更长的说明仍可用 vimg <子命令> --help。";

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
    after_help = HELP_BODY
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
