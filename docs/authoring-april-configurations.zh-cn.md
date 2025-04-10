
# APRIL 配置文件的编写
安同软件包重组信息列表（简称 APRIL）是一种用于半自动解决打包质量较差的 `.deb` 包的安装问题的数据描述格式。

## 基本格式
APRIL 的完整列表文件会以 JSON 格式分发。开发者若需编写 APRIL 配置文件，可使用更简洁的 TOML 格式文件。
要了解 TOML 语法，您可以阅读此处对 TOML 格式的简要概述：<https://toml.io/cn/>。

您需要在文件开头声明要修补的软件包：
```toml
schema = "0"  # APRIL 格式版本，目前必须为 '0'
name = "package"  # 要修补的包名称
compatible_versions = "*"  # 配置文件兼容的包版本范围
```

### 版本匹配语法
版本匹配语法应足够处理大多数情况。
最简单的匹配语法是 `"*"`，表示“任何版本”。这个表达式表示此配置适用于指定包的所有版本。
例如，如果您希望配置文件仅匹配版本号从 `1.0.0` 到 `1.1.0` 的包，则可以写为 `“>= 1.0.0 && < 1.1.0”`。
如果包在内容更改后不更改版本号，您还可以使用 `sha256sum(...)` 来匹配包。例如，`sha256sum(0000abcd) || 2.0.0` 将匹配 SHA256 校验和为 `0000abcd` 的包或版本为 `2.0.0` 的包。
您还可以使用 `==` 运算符精确匹配版本号。例如，`== 2.1.1+b3` 仅匹配版本号精确为 `2.1.1+b3` 的包。

### 完全转换
如果某个包的打包质量差到您希望丢弃其所有元数据，可以设置 `total_conversion = true` 以删除包的所有元数据。

## 覆盖
覆盖是用于“修正”或“修复”包中错误数据的方式。
以下是可用的基本覆盖选项：
- `name`：重命名包
- `version`：调整包的版本号
- `arch`：重新指定包兼容的架构
- `essential`：重新指定包是否对系统至关重要（即用户是否应被禁止删除此包？）
- `installed_size`：（整数）指定包的安装大小（安装后占用的空间）
- `section`：重新指定包应所属的分区
- `description`：重新编写包的描述
以下数组覆盖选项可用：
- `depends`：调整包的强制依赖关系
- `recommends`：调整包的推荐依赖关系
- `suggests`：调整包的建议依赖关系
- `pre_depends`：调整包的预安装依赖关系（罕用）
- `breaks`：调整包的依赖冲突（安装此包会导致哪些包无法正常工作？）
- `conflicts`：调整包的冲突（安装本包时会与哪个包冲突？）
- `replaces`：调整包的替换关系（安装本包时会替换哪个包？）
- `provides`：调整包提供的内容（安装本包时会为哪个包提供/打包内容？）
对于数组覆盖，您可以对每个定义进行差异化替换。例如，如果一个包包含 `a b c` 三个依赖项，您想移除 `b`，可以将 `depends = ["a", "c"]` 改为 `depends = ["-b"]`。如果您想添加 `d` 同时移除 `c`，可以将 `depends = ["a", "c"]` 改为 `depends = ["+d", "-c"]`。

### 覆盖安装脚本
有时，为了确保与 AOSC OS 的兼容，您会需要替换包中的安装脚本。
以下安装脚本可以被替换：
- `prerm`：预卸载脚本。在实际卸载过程开始前运行。
- `preinst`：预安装脚本。在实际安装过程开始前运行。
- `postrm`：卸载后脚本。在卸载过程完成后运行。
- `postinst`：安装后脚本。在解压过程完成后运行。
- `triggers`：触发器定义。定义本包的安装脚本是否应在其他包发生文件更改后运行。
`prerm`、`preinst`、`postrm` 和 `postinst` 均应为 Bash 脚本，而 `triggers` 脚本应遵循 Debian 政策手册中描述的格式：<https://manpages.debian.org/bookworm/dpkg-dev/deb-triggers.5.en.html>。

### 覆盖配置文件列表
部分包可能未正确声明其配置文件，导致在重新安装或升级包时，`dpkg` 可能错误覆盖用户的修改。
APRIL 通过定义 `conffiles = [“path/to/config/file”]` 提供了一个简单的解决方法。

## 移动文件
对于部分包，可能需要移动文件以解决文件系统布局问题。
您可以在 `files` 表中定义文件操作以实现此目的。
以下文件操作可用：
- `remove`：删除指定文件。
- `move`：将指定文件移动到另一个位置（在 `arg` 参数中）。
- `copy`：将指定文件复制到另一个位置（在 `arg` 参数中）。
- `link`：为指定文件创建符号链接到另一个位置（在 `arg` 参数中）。
- `patch`：将文本补丁应用到指定文件（在 `arg` 参数中）。
- `binary-patch`：将 xdelta3 编码的二进制补丁应用到指定文件（在 `arg` 参数中）。
- `track`：将指定文件标记为 dpkg 管理（即在卸载时会删除该文件）。
- `add`：在指定位置创建新文件（若文件已存在则失败），使用指定内容（在 `arg` 参数中）。
- `overwrite`：与 `add` 相同，但若文件已存在则覆盖原文件。
- `chmod`：修改指定文件的权限位（新权限位在 `arg` 参数中定义）。
- `mkdir`：在指定位置创建新目录。
默认情况下，所有文件操作都会使新文件被 `dpkg` 跟踪。目前无法“取消跟踪”文件以避免错误。
可选地，您可以定义 `phase` 参数来控制文件操作发生的时间。目前仅支持 `unpack`（在 `dpkg` 提取文件后）和 `postinst`（在 `dpkg` 运行 `postinst` 脚本后）。

## 示例
以下是一个带注释的完整示例，展示如何使用 APRIL：
```toml
schema = "0"
name = "sunloginclient"
compatible_versions = "*"

[overrides.scripts]
# 覆盖默认的 post-install 脚本以避免系统默认的 post-install 脚本
# 抛出 ‘不支持的操作系统错误’
postinst = “”"#!/bin/bash
# 终止所有还在运行的 sunloginclient 进程
killall sunloginclient > /dev/null 2>&1
# 清理日志文件
if [ -d '/var/log/sunlogin' ]; then
  rm -rf /var/log/sunlogin
fi
install -dm777 /var/log/sunlogin
chmod 666 /usr/local/sunlogin/res/skin/*.skin
chmod 766 /usr/share/applications/sunlogin.desktop
chmod 644 /usr/local/sunlogin/res/icon/*
systemctl enable runsunloginclient.service --now
“”"
# 将 `/etc/orayconfig.conf` 作为配置文件
# 以避免 dpkg 在系统升级时覆盖该文件
conffiles = ["/etc/orayconfig.conf"]
prerm = """#!/bin/bash
systemctl disable runsunloginclient.service --now
"""

[files]
# 将文件复制到正确位置
# 此操作相当于 `cp /usr/local/sunlogin/scripts/runsunloginclient.service /etc/systemd/system/runsunloginclient.service`
"/usr/local/sunlogin/scripts/runsunloginclient.service" = { action = "copy", arg = "/etc/systemd/system/runsunloginclient.service" }
```
