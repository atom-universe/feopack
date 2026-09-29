<div align="center">

# feopack

[![Rust](https://img.shields.io/badge/Rust-1.70+-orange.svg)](https://www.rust-lang.org/)
[![Node](https://img.shields.io/badge/Node-%3E%3D16-green.svg)](https://nodejs.org/)
[![TypeScript](https://img.shields.io/badge/TypeScript-5.0+-blue.svg)](https://www.typescriptlang.org/)

</div>

## 📖 关于

✨ 本项目为学习 Rspack 的核心功能实现原理以及项目工程架构而编写。

我也在博客里记录了这个项目的学习过程：[Feopack: Mini Rspack](https://furrycoder.com/posts/feopack/)。

### 为什么叫 FeO？

- **Rust** 意为"铁锈"。
- **FeO** 也是铁锈，但是锈化程度不高，这就有了一层 mini rspack 的含义。


## 🚀 快速开始

### 安装依赖

```bash
pnpm i 
```

### 构建项目

这里用 just 作为命令管理器封装了脚本，最终直接用 pkg script 即可使用：

```bash
# 在新机器上跑才发现 just 不是 npm 包...
brew install just

# 全量构建
pnpm run build

# 跳过 js 层，只构建 rust 部分
pnpm run build:update
```

### 使用

```typescript
import { version, compile } from 'feopack';

console.log(version());
console.log(compile('src/index.ts'));
```

## 🛠️ 如何开发

### 核心源码位置

> 现在 agent 时代了应该也没人会看这玩意儿
> 不过仔细想想其实 agent 时代之前也没人看我代码，所以好像也无所谓了

feopack 的核心代码有两个部分，分别是 ts 外壳和 rust 内核

其中 ts 外壳的入口是 `packages/feopack/index.ts` 
而 rust 内核的入口是 `crates/feopack/lib.rs`

二者通过 `napi-rs` 提供的 binding 层通信实现 FFI

### 代码调试流程

> 原本是放到 `__test__`, 但是懒得自己写测试样例，
> 所以还是改造了一下，使用 rspack 那复制过来的测试用例
> 它们放在 `packages/playground/cases`

可以这样验证测试用例：

```bash
# 测试所有用例（暂时不推荐使用，有的案例还没改造适配
pnpm test

# 指定使用特定的用例（也就是相对 packages/playground/cases 的路径
pnpm test chunk/basic
```

产物会出现在各个 case 目录下的 dist（如未配置输出目录）中。大部分 case 仍以人工观察为主；如果 case 目录包含 `verify.js`，playground runner 会在构建后执行它，完成该 case 的集成断言。

> 虽然不是方便，但是够用了

## 🧩 当前能力对比

标记说明：

- ✅ 已实现：当前已经支持；如果是简化实现，但覆盖主要学习 case，也算已实现
- 🚧 计划实现：当前还没有完整实现，但后续会继续推进
- ❌ 暂无计划：当前阶段暂时不会做

| 能力 | Rspack | Feopack | 备注 |
| --- | --- | --- | --- |
| Rust 编译核心 | ✅ | ✅ | Feopack 已经有 Rust 侧 compiler 主流程，但规模远小于 Rspack |
| TS/Node 外壳 | ✅ | ✅ | 通过 `napi-rs` 连接 JS 和 Rust |
| 基础构建生命周期 | ✅ | ✅ | Feopack 已整理出 `build -> compile -> make -> seal -> emit` |
| Module Graph | ✅ | ✅ | Feopack 支持静态 import 的简化模块图构建 |
| Chunk Graph | ✅ | ✅ | Feopack 当前是简化版，主要把模块组织到单个 chunk |
| Code Generation | ✅ | ✅ | Feopack 已能生成可运行 bundle，但 runtime 和产物结构都较简化 |
| Rust Loader | ✅ | ✅ | 支持内置 Rust loader |
| JS Loader Bridge | ✅ | ✅ | 已支持从 Rust 调用 Node 侧 JS loader runner |
| Loader pitch / normal | ✅ | ✅ | Feopack 已支持简化版 pitch / normal 执行流 |
| Inline Loader Request | ✅ | ✅ | 已支持类似 `-!loader!resource` 的基础解析 |
| Plugin 基础机制 | ✅ | ✅ | 已有简化的 `Plugin`、`PluginDriver`、compiler hooks 和 tap 调度 |
| JS Plugin 兼容 | ✅ | ✅ | 支持 `plugin.apply(compiler)` 和部分常用 compiler hooks，已跑通一个 npm plugin case |
| Watch Mode | ✅ | ✅ | 使用 Watchpack 监听 compilation 的文件依赖；当前每次变化仍执行完整构建 |
| Incremental Rebuild | ✅ | 🚧 | 暂未实现 |
| Compilation 级 Hooks | ✅ | 🚧 | 等真实 plugin case 需要时再补，例如 `processAssets` |
| HMR | ✅ | 🚧 | 暂未实现 |
| Persistent Cache | ✅ | 🚧 | 暂未实现 |
| Source Map | ✅ | 🚧 | 暂未实现完整 source map 支持 |
| Tree Shaking | ✅ | 🚧 | 暂未实现 |
| Code Splitting | ✅ | 🚧 | 暂未实现完整 chunk splitting |
| Module Federation | ✅ | ❌ | 当前学习阶段暂不计划实现 |
| 生产级 Resolver | ✅ | ❌ | Feopack 只会保留学习所需的简化解析能力 |
| 完整 Webpack/Rspack 配置兼容 | ✅ | ❌ | Feopack 不追求完整兼容，只挑核心机制学习 |

当前建议推进顺序是：先完成 Compiler hooks 的最小闭环，再实现 Watch Mode 和最小增量重建，最后根据真实 plugin case 补 Compilation hooks。

Compiler hooks 和 Compilation hooks 不是能力等级，而是归属不同：Compiler hooks 描述整次构建的阶段，例如 `beforeRun`、`make` 和 `done`；Compilation hooks 描述一次编译内部对 module、chunk 和 assets 的处理，例如 `buildModule` 和 `processAssets`。Feopack 当前只实现了前者的最小子集，后者等实际 case 需要时再补。

## 📝 说明

这是一个**学习性质**的项目，请勿用于实际生产（相信也没人会这么做）。


## 参考资料

感谢所有相关的公开资料的创作者们的努力！

> 关于这部分，我整理成了文档: https://ai.feishu.cn/wiki/U4imwSfmFimoeGk1qQrck2e5nog?from=from_copylink


## 📄 License

MIT
