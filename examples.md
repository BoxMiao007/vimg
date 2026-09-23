# vimg examples
Note: Support for animated avif isn't everywhere yet, try viewing with chrom{e,ium}.

## Example: Animated 1440p realtime vcs
The default args produces a realtime animated avif of 1.5s length.

```sh
vimg vcs -c5 -n25 -H288 bbb.mkv
```

-> [bbb.1440p.avif](bbb.1440p.avif)

## Example: Animated 1080p vcs at half-speed
Setting the height for 1080p output and using default args with a 10fps 
frame rate makes a 3s half-speed animation.

```sh
vimg vcs -c5 -n25 -H216 --avif-fps=10 bbb.mkv
```
-> [bbb.1080p.avif](bbb.1080p.avif)

## Example: Animated 1440p realtime vcs
The default args produces a realtime animated avif.

```sh
vimg vcs -c5 -n25 -H288 bbb.mkv
```

-> [bbb.1440p.avif](bbb.1440p.avif)

## Example: Still ultrawide vcs
Using a single capture frame `-f1` results in a still/static vcs. 
7 columns & 35 captures results in close to 21:9 ultrawide resolution grid.

```sh
vimg vcs -c7 -n35 -H288 -f1 bbb.mkv
```

`--no-info` leaves the grid bare. `--info-all` adds bitrate, pixel format, frame count, container and every audio track. `join` prints the same header when given `--video`.

## Example: Layout 1

One fixed band of 14 cells: four columns and five row units. The top-left and bottom-right cells are twice as wide and tall as the others, with a full row of four small cells between them. An 8px black gap separates the cells, and the same gap sits on the left and right edges. `-n 2` stacks a second band underneath, with another row of four small cells between the two large cells. `-c`, `-H` and `-W` are ignored.

```sh
vimg vcs --layout 1 -n 2 bbb.mkv
```

-> [bbb.wide.avif](bbb.wide.avif)
