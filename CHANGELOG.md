# Unreleased
* `vcs` 与 `join` 增加 `--layout 1`。一截 19 格，左上和右下是大格，其余是小格；`-n` 在这个版式下表示往下排几截，默认一截。列数和格子尺寸固定，另外写的 `-c`、`-H`、`-W` 会被忽略并提示。
* 修复参数栏字形变成实心方块。覆盖率原先只写进 alpha，网格存成 BMP 时 alpha 被丢掉，包围盒里每个像素都变成纯白。
* `vcs` 默认在网格上方绘制参数栏：文件名、大小、分辨率、解码器、时长。`--no-info` 关闭，`--info-all` 追加码率、像素格式、总帧数、容器和每一条音轨。
* `join` 增加 `--video`。给出后绘制同一块参数栏；`--info-all` 必须同时给出 `--video`。
* `--font` 与环境变量 `VIMG_FONT` 指定参数栏字体。找不到中文字体时改用英文标签，并在终端警告。

# v0.2.0
* Use svt-av1 to encode avifs instead of aom-av1, speeds up encoding.
* By default use svt-av1 preset 6 for multi-frame avifs.
* Add `vcs` option `--avif-codec VCODEC` for specifying the ffmpeg vcodec for encoding the output avif,
  e.g. use vcodec "libaom-av1" for more like the old behaviour.
* Change default `--avif-fps=20` (was 10) meaning default args will yield real time avifs.

# v0.1.4
* Update dependencies.

# v0.1.3
* Fix `vcs` `-W` ffmpeg vfilter bug.
* Fix label background pixel oob panic.

# v0.1.2
* Cleanup temp dir on error / ctrl-c.

# v0.1.1
* Add `print-completions` command.

# v0.1.0
* Add `vcs`, `extract`, `join` commands.
