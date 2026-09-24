# vimg

单二进制 CLI（edition 2024）。`vcs` 用 ffmpeg 抽帧、拼网格，再编码成 AVIF 接触表。子命令：`vcs`、`extract`、`join`、`print-completions`。

## 验证

- 与 CI 一致：`cargo fmt -- --check`，然后 `cargo test --locked`。仓库没有 `#[test]`；`cargo test` 主要确认 `--locked` 能通过。
- 补全冒烟（CI 同款）：`cargo run --locked -- print-completions bash`（`fish`、`zsh` 同理）。
- 跑 `extract` / `vcs` 需要 PATH 里的 `ffmpeg` 和 `ffprobe`。样例视频：`test/bbb-test-video.mp4`。
- `./deploy` 是作者本机脚本：nightly `fmt --check`、release 构建，并复制到 `~/bin/vimg`。CI 用 stable `cargo fmt`。不要把它当构建入口。

## 行为

- 默认 AVIF 编码器是 `libsvtav1`。仅 `libaom-av1` 传 `-cpu-used`，其余编码器传 `-preset`。
- `vcs` 默认在网格上方画参数栏。`--no-info` 关闭，`--info-all` 写完整参数。字体用 `--font` 或 `VIMG_FONT`，否则用内嵌 MiSans。`join` 只有给出 `--video` 才画。
- `-l 1`（`--layout 1`）是固定的 14 格一截：4 列、5 个行单位，左上和右下各一个 2×2 大格，正中间一行是 4 个小格。左上从左到右再换行；右下大格左边先从左到右，再换到下一行，大格最后放。格与格之间留 8 像素黑缝，大格盖住内部那条缝，左右外缘留同样宽的一条，上下外缘不留。从第二截起，截与截之间多一行 4 个小格，把上下大格隔开。`vcs` 的 `-n` 在这个版式下是截数，默认 1。不写 `-l` / `--layout` 仍是等大网格。
- `vcs` 输出扩展名必须是 `.avif`。`-f` 默认：`extract` 为 1，`vcs` 为 30。
- 临时目录是当前目录（或 `--output-dir`）下的 `.vimg-<12 位>`。进程退出和 Ctrl-C 时删除，除非 `--keep`。不要提交。

## 易错点

- `[lints.rust] unused_crate_dependencies = "deny"`：未使用的 crate 依赖会让构建失败。
- 参数栏字体是 `src/command/header/MiSans-Regular.ttf`，由 `header.rs` 的 `include_bytes!` 嵌入。时间戳字体是 `src/command/join/Cantarell-Regular.ttf`，由 `label.rs` 嵌入。移动文件须同步路径。
- `Cargo.toml` 的 `exclude` 含 `**.avif` 和 `.github`：示例 AVIF 与 CI 不会打进发布的 crate。
