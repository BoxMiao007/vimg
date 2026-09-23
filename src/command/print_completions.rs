use clap::Parser;
use clap_complete::Shell;

/// 把 shell 补全脚本打到标准输出。
///
/// 不写 shell 时是 bash。把输出保存或交给对应 shell 加载即可。
#[derive(Parser)]
#[group(skip)]
#[command(
    override_usage = "vimg print-completions [shell]",
    after_help = "示例:\n  \
    vimg print-completions bash\n  \
    vimg print-completions fish\n  \
    vimg print-completions zsh\n  \
    vimg print-completions powershell"
)]
pub struct PrintCompletions {
    /// 要生成补全的 shell。
    #[arg(value_enum, value_name = "shell", default_value_t = Shell::Bash)]
    shell: Shell,
}

impl PrintCompletions {
    pub fn run(self) {
        clap_complete::generate(
            self.shell,
            &mut crate::Command::command(),
            "vimg",
            &mut std::io::stdout(),
        );
    }
}
