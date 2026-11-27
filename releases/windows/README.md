# KROMA for Windows

原生 Windows 版，不需要 Docker，也不需要 WSL。

## 怎么用

**双击 `kroma-server.exe`** 就行：没有黑窗，图标出现在系统托盘（右下角）。

- 双击托盘图标 → 打开浏览器界面
- 右键托盘图标 → Open KROMA / Stop KROMA
- Stop 是正常关闭，会等模块收尾，不是强杀

浏览器地址：http://localhost:4041

首次打开会让你创建管理员账户，然后建媒体库、指定媒体文件夹。

想边跑边看日志，用 `start.bat`：它有控制台窗口，关掉窗口就停服务。

## 文件夹说明

```
kroma-server.exe    主程序（双击即托盘运行）
start.bat           控制台启动器（调试用，能看到日志）
web\                网页界面
bin\ffmpeg.exe      内置转码（HLS 回退、预览图）
bin\ffprobe.exe     内置元数据探测
data\               你的数据（数据库、日志、已安装模块）——备份这个目录
```

程序认自己所在的文件夹：`data`、`web`、`bin` 都相对 exe 解析，整个目录可以搬到任何位置（U 盘、别的机器都行），不依赖当前工作目录。

## 改端口 / 媒体目录

不需要改文件，用环境变量即可。建一个快捷方式，在「目标」前加：

```
cmd /c set KROMA_PORT=8080 && set KROMA_MEDIA_DIRS=D:\Movies,D:\Shows && "E:\kroma\kroma-server.exe"
```

- `KROMA_PORT` 默认 4041
- `KROMA_MEDIA_DIRS` 逗号分隔；也可以进界面后在「服务器」里加媒体库

## 已内置

- 115 云盘模块（tv.kroma.cloud115），在「模块」页可配置账号
- ffmpeg / ffprobe，转码与元数据开箱即用

## 注意

- 数据全在 `data\`，搬机器时整个文件夹复制走即可。
- 日志在 `data\logs\`，按天滚动，保留 30 天。无控制台时日志只写文件。
- Passkey（指纹/人脸登录）在 Windows 版未启用，用密码登录。
- 托盘图标建不出来时（比如以服务方式运行、没有交互桌面），服务照常运行，只是没有图标。
