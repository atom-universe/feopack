import { FeopackOptions } from '.'
import * as binding from '@feopack/binding'
import { getRawOptions } from './config/adapter'
import { Compilation } from './Complication'
import {
  createDefaultJsLoaderDispatcher,
  registerJsLoaderDispatcher,
} from './loader-runner/dispatcher'
import { runJsLoaders } from './loader-runner/snapshot'
import { JsLoaderState } from './loader-runner/types'
import { CompilerHooks, createCompilerHooks } from './hooks'
import { Watching, WatchHandler, WatchOptions } from './Watching'

export class Compiler {
  #inner?: binding.Rspack // Rust 那一侧对应的 Compiler
  // 打包的根路径
  options: FeopackOptions
  context: string
  compilation?: Compilation
  watching?: Watching
  modifiedFiles?: ReadonlySet<string>
  removedFiles?: ReadonlySet<string>
  hooks: CompilerHooks
  webpack: Record<string, never>
  rspack: Record<string, never>

  constructor(options: FeopackOptions) {
    this.context = options.context
    this.options = options
    this.hooks = createCompilerHooks()
    this.webpack = {}
    this.rspack = {}
    registerJsLoaderDispatcher(createDefaultJsLoaderDispatcher())
    this.#applyPlugins()
  }

  #applyPlugins() {
    for (const plugin of this.options.plugins ?? []) {
      if (typeof plugin === 'function') {
        ;(plugin as (this: Compiler, compiler: Compiler) => void).call(this, this)
        continue
      }
      plugin.apply(this)
    }
  }

  #getInner(): binding.Rspack {
    if (this.#inner) {
      return this.#inner
    }
    const rawOptions = getRawOptions(this.options)
    // 拿到 rust 那一侧的 compiler 实例
    const instanceBinding = require('@feopack/binding')

    // 把 js loader runner 传递给 rust 那一侧
    this.#inner = new instanceBinding.Rspack(
      rawOptions,
      async (ctx: binding.JsLoaderContextInput) => {
        try {
          // 其实这个玩意儿的核心实现就是直接 fork 的 webpack 的源码
          // 这里其实就是之前说的，能够逆向让 rust 调用 js 的操作的体现之一
          const result = await runJsLoaders({
            loaderState:
              ctx.loaderState === 'pitching'
                ? JsLoaderState.Pitching
                : JsLoaderState.Normal,
            loaders: ctx.loaders,
            resource: ctx.resource,
            source: ctx.source,
            projectRoot: ctx.projectRoot,
            skipReadResource: ctx.skipReadResource,
          })
          return {
            source: result.source,
            shortCircuit: result.shortCircuit,
            pitchedLoaderIndex: result.pitchedLoaderIndex,
          }
        } catch (err) {
          const message = err instanceof Error ? err.message : String(err)
          throw new Error(`feopack js loader: ${message}`)
        }
      },
      // Rspack 为每个 native hook 注册单独的 JS tap adapter；这里压成一个事件回调。
      async (event: {
        name: string
        filename?: string
        targetPath?: string
      }) => {
        const compilation = this.compilation
        if (!compilation) {
          throw new Error(`feopack js hook ${event.name}: compilation is not ready`)
        }

        switch (event.name) {
          case 'thisCompilation':
            await this.hooks.thisCompilation.promise(compilation, {})
            break
          case 'compilation':
            await this.hooks.compilation.promise(compilation, {})
            break
          case 'make':
            await this.hooks.make.promise(compilation)
            break
          case 'emit':
            await this.hooks.emit.promise(compilation)
            break
          case 'assetEmitted':
            await this.hooks.assetEmitted.promise(event.filename, {
              targetPath: event.targetPath,
            })
            break
          case 'afterEmit':
            await this.hooks.afterEmit.promise(compilation)
            break
        }
      },
    )

    return this.#inner!
  }

  async #build() {
    // rust, 启动！
    const inner = this.#getInner()
    const isRebuild = this.compilation !== undefined
    this.compilation = new Compilation(this, inner)
    if (isRebuild) {
      await inner.rebuild(
        Array.from(this.modifiedFiles ?? []),
        Array.from(this.removedFiles ?? []),
      )
    } else {
      await inner.build()
    }
    return this.compilation
  }

  async compile() {
    const params = {}
    await this.hooks.beforeCompile.promise(params)
    this.hooks.compile.call(params)
    const compilation = await this.#build()
    await this.hooks.afterCompile.promise(compilation)
    return compilation
  }

  async run() {
    try {
      await this.hooks.beforeRun.promise(this)
      await this.hooks.run.promise(this)
      const compilation = await this.compile()
      const stats = compilation.getStats()
      await this.hooks.done.promise(stats)
      this.hooks.afterDone.call(stats)
      return stats
    } catch (err) {
      this.hooks.failed.call(err)
      throw err
    }
  }

  watch(watchOptions: WatchOptions, handler: WatchHandler): Watching {
    if (this.watching) {
      throw new Error('Concurrent watch is not supported')
    }
    this.watching = new Watching(this, watchOptions, handler)
    return this.watching
  }
}
