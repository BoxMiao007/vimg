# vimg

从视频抽出截帧、排成网格、再编码成 AVIF 接触表的命令行工具。依赖 `ffmpeg` 与 `ffprobe`。

输出可以是静图，也可以是多帧动画。默认编码器是 `libsvtav1`。

## 子命令

### vcs

从视频生成接触表。抽帧、拼网格，再编码成 `.avif`。输出扩展名必须是 `.avif`，不写 `-o` 时用输入文件名换扩展名。

```text
vimg vcs [选项] -c <列数> -H <格子高度> -n <格数> <视频>
```

等大网格必须给出 `-c` 和 `-H`（或 `-W`，二者取一），以及 `-n`。

网格上方默认画参数栏：文件名、大小、分辨率与帧率、解码器、时长。文字左对齐。

- `--no-info` 不画参数栏。
- `--info-all` 追加码率、像素格式、总帧数、容器和每一条音轨。
- `--font` 或环境变量 `VIMG_FONT` 指定字体文件。按指定文件、MiSans、系统中文字体的顺序找；都没有就改用英文标签，并在终端警告。

`-f` 默认 30，所以默认输出是约 1.5 秒的动画（每格 30 帧，`-t` 默认 1500ms，`--avif-fps` 默认 20）。`-f 1` 是静图。

更多例子见 [examples.md](examples.md)。

### 版式 1

`--layout 1` 不用等大网格，改用固定的一截 14 格：

- 4 列、5 个行单位。左上和右下各一个 2×2 大格，中间一行是 4 个小格。
- 格与格之间留 8 像素黑缝。大格盖住自己内部那条缝。
- 左右外缘各留 8 像素，上下外缘不留。
- 从第二截起，两截之间多一行 4 个小格，把上一截的右下大格和下一截的左上大格隔开。所以 1 截 14 张，2 截 32 张，3 截 50 张。
- 这时 `-n` 表示截数，默认 1。`-c`、`-H`、`-W` 被忽略，并在终端提示。
- 小格高度固定 216，宽度按画面比例换算。大格是两个小格再加中间那条缝。
- 每一格右下角印采样时刻。字号约为该格短边的 16%，最大 40 像素。没有底色，靠描边和阴影分开。

```sh
vimg vcs --layout 1 -n 2 视频.mkv
```

不写 `--layout` 仍是等大网格，格子贴在一起，没有外缘空白。

### extract

用 ffmpeg 从视频抽出 BMP 截帧。`-n` 必填，`-f` 默认 1。

```text
vimg extract [选项] -n <格数> <视频>
```

### join

把同样尺寸的截帧拼成一张网格图。给出 `--video` 才画和 `vcs` 一样的参数栏；`--info-all` 必须同时给出 `--video`。

```text
vimg join [选项] -c <列数> -o <输出> <图片>...
```

版式 1 下，图片张数必须对得上截数：14、32、50……对不上就不出图。

### print-completions

打印 shell 补全脚本。

```sh
vimg print-completions bash
vimg print-completions fish
vimg print-completions zsh
```

## 安装

需要 PATH 里有不太旧的 `ffmpeg` 和 `ffprobe`。

用 cargo，从本仓库安装：

```sh
cargo install --git https://github.com/BoxMiao007/vimg
```

或在仓库里构建发布版：

```sh
cargo build --release
```

二进制在 `target/release/vimg`。

## 其他

- 仅 `libaom-av1` 传 `-cpu-used`，其余编码器传 `-preset`。单帧默认 preset 1，多帧默认 6。
- 临时目录是当前目录（或 `--output-dir`）下的 `.vimg-<12 位>`。进程退出和 Ctrl-C 时删除，除非 `--keep`。
- 维护所用的 Rust 版本见 [latest stable rust](https://gist.github.com/alexheretic/d1e98d8433b602e57f5d0a9637927e0c)。
