# 1. 基本概念

crv-cli 是 Chronoverse 的命令行工具，程序名称为 `crv`。通过 `crv` 可以对客户端 (crv-edge) 和服务端 (crv-hive) 进行管理，并完成文件版本管理相关的一切操作。

## 1.1 仓库路径 (Depot Path)

服务端存储受管理文件的逻辑实体被称作 depot，depot 采用树形结构管理文件的相对关系，每个文件在 depot 中的路径被称作 depot path。在给定服务端地址的情况下，depot path 可以用于索引受管理的文件。

在不同的应用场景下，我们可能想要精确索引一个文件或是范围索引多个文件，因此出现了三种不同的 depot path：

- 单文件 depot path：索引一个文件
- 范围索引 depot path：索引多个文件
- 正则 depot path：索引多个文件

### 1.1.1 单文件 depot path

语法为：`//(<dir>/){0+}<filename>`

其中：

- `<dir>`：单个层级的目录名，例如 `asset`
- `<filename>`：文件名，例如 `logo.png`

### 1.1.2 范围索引 depot path

语法为：`//(<dir>/){0+}(...){0-1}<filename wildcard>`

其中：

- `<dir>`：单个层级的目录名，例如 `asset`
- `...`：递归目录通配符，用于匹配零到多个层次的任意名称的目录
- `<filename wildcard>`：文件名通配符，有三种形式
  - 确切的文件名
  - 后缀名统配，语法为 `~<ext name>`，例如 `~png`、`~jpg`、`~obj.meta`，用于统配某个后缀名的所有文件
  - 空，匹配所有文件

### 1.1.3 正则 depot path

语法为：`r://<regex>`

其中：

- `<regex>`：一个正则表达式，将会匹配所有符合这个正则表达式的单文件 depot path（严谨地说，是单文件 depot path 除掉首部的 `//` 后余下的部分）对应的文件，正则表达式风格同 `regex` crate

> **版本描述符 (revision descriptor)**
>
> 有的时候，需要索引指定文件的指定版本，此时可以使用版本描述符，版本描述符有以下几种格式：
> - `#<revision id>`，例如 `#33` ，用于表示某个版本的文件
> - `@<changelist number>`，例如 `@3588021`，用于表示某个 changelist 刚刚提交后的文件或路径的版本状态
>
> 通常来讲，在能够使用版本描述符的场景下，省略版本描述符即可表示文件或路径的最新版本状态

## 1.2 Depot Tree

Chronoverse 采用 depot tree 的概念作为分支模型的基础。一个 depot tree 是整个 depot 目录树的一棵子树，亦即一个子目录。

对于如下 depot 目录树：

```
project
├── 1.0
│   ├── 1.0.1
│   │   ├── assets/...
│   │   └── src/...
│   └── 1.0.2
│       ├── assets/...
│       └── src/...
└── 1.1
    ├── 1.1.1
    │   ├── assets/...
    │   ├── src/...
    │   └── test/...
    └── 1.1.2
        ├── assets/...
        ├── src/...
        └── test/...
```

以下几个目录树都可以被称作 depot tree：

```
project
└── 1.0
    ├── 1.0.1
    │   ├── assets/...
    │   └── src/...
    └── 1.0.2
        ├── assets/...
        └── src/...
```

```
project
└── 1.0
    └── 1.0.2
        ├── assets/...
        └── src/...
```

而下面的目录树不是 depot tree：

```
project
└── 1.0
    ├── 1.0.1
    │   └── src/...
    └── 1.0.2
        └── src/...
```

```
project
├── 1.0
│   └── 1.0.1
│       ├── assets/...
│       └── src/...
└── 1.1
    └── 1.1.1
        ├── assets/...
        ├── src/...
        └── test/...
```

## 1.2 工作区 (Workspace)

工作区是客户端管理的存储在用户本地的 depot 文件树的任意**子集**的副本，用户可以对工作区中的文件进行变更并提交到服务器上，从而影响 depot 文件树。

工作区中文件与 depot 文件树之间的映射关系可以由若干个非正则 depot path 到工作区根目下的排除/包含映射来表示，例如：

```
//project/1.0/1.0.1/... //1.0.1  # 包含映射，`//`用于代表工作区根目录
-//project/1.0/1.0.1/...~meta  # 排除映射
```

## 1.3 文件状态

- 未追踪 (untracked)
- 忽略 (ignored)
- 已追踪 (tracked)
  - 干净 (clean)
  - 已修改 (modified)：对文件执行 `crv add`、修改文件内容以及删除文件均会使文件进入“已修改”状态
    - 独占修改 (exclusive)
    - 共享修改 (shared)：没有任何一个工作区独占修改这个文件，称为共享修改
    - 过时 (stale)：远端最新版本没有被拉新到本地，但当前工作区仍然修改了这个文件，过时的文件无法提交
    - 篡改 (tampered)：在没有过时的情况下，已经有其他工作区独占修改了，但当前工作区仍然修改了这个文件，称为篡改；如果此时其他工作区释放了独占修改，则文件状态回到共享修改，仍能提交
  - 冲突 (conflict)

## 1.4 变更列表 (Changelist)

Changelist 分为本地 Changelist 和服务端 Changelist。

TODO

# 2. 工作流程

## 2.1 客户端用户工作流程

设有如下 depot：

```
[项目名称]
├── 00_Reference_参考资料
│   ├── 00.1_Admin_行政与法务
│   ├── 00.2_Finance_财务
│   ├── 00.3_Contact_联系人
│   └── 00.4_Archived_归档
│
├── 01_Development_开发
│   ├── 01.1_Concept_概念
│   ├── 01.2_Treatment_故事大纲
│   ├── 01.3_Scripts_剧本
│   │   ├── 01.3.1_Drafts_草稿
│   │   └── 01.3.2_Locked_定稿
│   └── 01.4_Pitch_提案
│
├── 02_Pre-Production_前期制作
│   ├── 02.1_Schedule_日程
│   ├── 02.2_Budget_预算
│   ├── 02.3_Casting_选角
│   ├── 02.4_Storyboards_分镜
│   ├── 02.5_Location_场地
│   ├── 02.6_Art_美术
│   └── 02.7_Technical_技术
│
├── 03_Production_拍摄制作
│   ├── 03.1_Footage_素材
│   │   ├── 03.1.1_Original_原始素材
│   │   └── 03.1.2_Proxy_代理文件
│   ├── 03.2_Dailies_每日样片
│   ├── 03.3_Reports_报告
│   └── 03.4_Stills_剧照
│
├── 04_Post-Production_后期制作
│   ├── 04.1_Editorial_剪辑
│   │   ├── 04.1.1_Project_工程文件
│   │   ├── 04.1.2_Edits_版本
│   │   └── 04.1.3_Graphics_字幕和图文
│   ├── 04.2_VFX_视觉特效
│   │   ├── 04.2.1_VFX_Shots_特效镜头清单
│   │   ├── 04.2.2_Renders_渲染文件
│   │   └── 04.2.3_Assets_特效资产
│   ├── 04.3_Sound_声音
│   │   ├── 04.3.1_Original_原始同期声
│   │   ├── 04.3.2_ADR_对白补录
│   │   ├── 04.3.3_SFX_音效
│   │   ├── 04.3.4_Music_音乐
│   │   └── 04.3.5_Mix_混音文件
│   └── 04.4_Color_调色
│       ├── 04.4.1_LUTs_查找表
│       └── 04.4.2_Final_Graded_调色后文件
│
└── 05_Delivery_交付
    ├── 05.1_Masters_母版
    ├── 05.2_Distributions_发行版
    ├── 05.3_Marketing_宣传材料
    └── 05.4_Documentation_文档
```

一个客户端用户想要修改交付阶段的宣传资料文件，在忽略掉注册用户、授权、登录服务器等与文件操作无关步骤后，该用户要经历如下几个步骤以完成这一任务。

### 2.1.1 创建工作区

用户首先需要使用 `crv workspace create` 创建如下映射的工作区，假设工作区名为 `marketing_workspace`，并切换工作目录至这个工作区的本地目录。

```
//[项目名称]/05_Delivery_交付/05.3_Marketing_宣传材料/... /home/project_name/05_Delivery_交付/05.3_Marketing_宣传材料
```

### 2.1.2 拉新文件

用户使用 `crv sync` 进行拉新，而后 `/home/project_name/05_Delivery_交付/05.3_Marketing_宣传材料` 将会出现此刻服务端上文件的最新版本。

### 2.1.3 修改宣传资料文件

用户可以直接编辑 `05.3_Marketing_宣传材料` 下的文件，或使用 `crv lock` 申请排它锁后修改文件。假设被修改的文件为 `05.3_Marketing_宣传材料/PPT/超前点映.pptx`。

### 2.1.4 创建 changelist

用户使用 `crv changelist create` 命令创建新的 changelist，假设创建了编号为 `3577` 的 changelist。

### 2.1.5 向 changelist 中添加文件

用户使用 `crv changelist 3577 append /home/project_name/05_Delivery_交付/05.3_Marketing_宣传材料/PPT/超前点映.pptx` 将被修改的文件添加到刚刚创建的 changelist 中。

### 2.1.6 提交 changelist

用户使用 `crv changelist 3577 submit` 将刚刚创建的 changelist 中的文件修改提交到服务端。

### 2.1.7 直接提交

如果用户不想按照 2.1.4 到 2.1.6 的流程创建 changelist 、添加文件而后提交 changelist，而是希望直接指定某些文件进行提交，那么 `submit` 系列命令正是为这种情况准备的。用户使用 `crv submit /home/project_name/05_Delivery_交付/05.3_Marketing_宣传材料/PPT/超前点映.pptx` 进行提交，Chronoverse 会跳过创建本地 changelist 这一步骤直接提交变更到服务端。

## 2.2 管理员工作流程

TODO

# 3. 指令手册

`crv` 指令的指令风格为：`<指令族> <谓语> <宾语> <状语>`。其中指令族可以理解为一个作用域，例如 2.1 下的几个小节中提到的 `crv workspace`、`crv changelist`、`crv changelist 3577`，当然也包括 `crv` 本身。指令族不是一个很严谨的概念，但它是设计 `crv` 指令时的重要指导之一。

3.1至3.12小节介绍了与客户端文件相关的若干指令，3.13以及之后的小节介绍了与日常管理相关的若干指令。

## 3.1 crv workspace

创建、删除、管理工作区。crv-cli 会根据当前工作目录判断当前工作区。例如，如果工作区 `marketing_workspace` 的根目录为 `/home/project_name/05_Delivery_交付/05.3_Marketing_宣传材料`，则当处在这个目录的任意子目录下的时候，当前工作区均为 `marketing_workspace`。

用法：

- `crv workspace create`：创建工作区，这一行为将会打开系统默认文本编辑器，以编辑工作区名称、文件映射等基本信息
- `crv workspace delete <workspace_name>`：删除一个名为 `<workspace_name>` 的工作区
- `crv workspace list`：列出所有工作区
- `crv workspace describe (<workspace_name>){0+}`：查看一个名为 `<workspace_name>` 的工作区的状态，不指定的情况下将会查看当前工作区的状态
- `crv workspace current`：查看当前工作区的状态

## 3.2 crv add

将未追踪的文件添加到 Chronoverse。

用法：

- `crv add (<file>|<dir>){1+}`：将指定范围内的所有未追踪文件添加到 Chronoverse。

## 3.3 crv sync

将服务端的文件同步到本地。

用法：

- `crv sync (<depot path>){0+}`：将服务端的文件同步到本地，如果不指定 depot path 则默认为工作区下被包含映射的所有文件。

状语：

- `--force`：强制覆盖本地文件，即使本地文件有修改。

## 3.4 crv lock

独占锁定某个文件，独占锁定后，当前工作区提交该文件前别的工作区无法提交这个文件。

用法：

- `crv lock (<file>|<dir>){1+}`：独占锁定指定范围内的已追踪文件。

## 3.5 crv changelist

创建、删除、管理 changelist。

用法：

- `crv changelist create`：创建一个 changelist。
- `crv changelist delete <changelist number>`：删除一个编号为 `<changelist number>` 的 changelist。
- `crv changelist list`：列出所有 changelist。
- `crv changelist describe <changelist number>`：查看一个编号为 `<changelist number>` 的 changelist 的状态。

`crv changelist describe <changelist number>` 具有状语：

- `--list-file` 查看 changelist 内的所有文件的简要状态。

## 3.6 crv revert

将某个本地文件还原回 clean 状态，如果被还原的文件已经被独占锁定，则会释放独占锁定。

用法：

- `crv revert (<file>|<dir>){1+}`：将指定范围内的所有已追踪文件还原回 clean 状态。

## 3.7 crv submit

提交某个已修改的文件。

- `crv submit (<file>|<dir>){1+}`：提交指定范围内的所有已修改的文件。

## 3.8 crv snapshot

用户可能希望将本地已修改的文件共享给他人，但还不想提交，此时可以通过 snapshot 功能将本地文件当前副本同步到服务端。这一功能在其他 VCS 中常被称作 shelve。但在 Chronoverse 中，这一行为不再与 Changelist 绑定，用户可以对指定文件创建任意多个快照。创建将会得到一个快照 id，其他用户可以用这个 id 来获取快照内容

- `crv snapshot create (<file>|<dir>){1+}`：根据路径范围创建一个快照。
- `crv snapshot create <changelist number>`：根据本地 Changelist 创建一个快照。
- `crv snapshot delete <snapshot id>`：删除一个名为 `<snapshot id>` 的快照。
- `crv snapshot list`：列出所有快照。
- `crv snapshot describe <snapshot id>`：查看一个名为 `<snapshot id>` 的快照的状态。
- `crv snapshot restore <snapshot id>`：下载一个名为 `<snapshot id>` 的快照内所有文件到本地工作区。

## 3.9 crv merge

合并某个分支上的文件到本地工作区。

用法：

- `crv merge <branch name>`：合并指定分支上的所有文件到本地工作区。
- `crv merge <branch name> <depot path without revision descriptor>`：合并指定分支上的指定文件到本地工作区。

## 3.10 crv resolve

将某个冲突文件的状态转移到已修改。

用法：

- `crv resolve (<file>|<dir>)`：将指定范围内的冲突文件的状态转移到已修改。

## 3.11 crv describe

列出某个文件的状态。

用法：

- `crv describe (<file>|<dir>)`：列出指定范围内的文件的状态。

---

## 3.12 crv branch

## 3.13 crv unlock

## 3.14 crv user

## 3.15 crv login

## 3.16 crv logout

## 3.17 crv perm
