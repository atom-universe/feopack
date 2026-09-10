/**
 * 简化自 Rspack 的 Watching：保留事件聚合、构建中失效和串行重建，
 * 暂时只监听 Compilation 实际读取的文件。
 */
import path from 'node:path'
// Watchpack 是 webpack/Rspack 用的文件监听库
// 封装不同平台的文件监听差异，并把短时间内连续发生的变化聚合成一批事件。
import Watchpack from 'watchpack'

import type { Compiler } from './Compiler'
import type { Stats } from './Stats'

export type WatchOptions = Watchpack.WatchOptions
export type WatchHandler = (error: Error | null, stats?: Stats) => void

export class Watching {
  readonly compiler: Compiler
  readonly watchOptions: WatchOptions

  #watcher: Watchpack
  #handler: WatchHandler
  #running = false
  #invalid = false
  #invalidReported = false
  #closed = false
  #changedFiles = new Set<string>()
  #removedFiles = new Set<string>()

  constructor(
    compiler: Compiler,
    watchOptions: WatchOptions,
    handler: WatchHandler,
  ) {
    this.compiler = compiler
    this.watchOptions = {
      // 这个就是之前遇到的一个防抖间隔参数了
      aggregateTimeout: 20,
      ignored: this.#defaultIgnored,
      ...watchOptions,
    }
    this.#handler = handler
    this.#watcher = new Watchpack(this.watchOptions)

    this.#watcher.on('change', (file, modifiedTime) => {
      this.#reportInvalid(file, modifiedTime)
    })
    this.#watcher.on('remove', (file) => {
      this.#reportInvalid(file, Date.now())
    })
    this.#watcher.on('aggregated', (changes, removals) => {
      this.#mergeChanges(changes, removals)
      this.#invalidate()
    })
    process.nextTick(() => this.#invalidate())
  }

  invalidate() {
    this.#reportInvalid(null, Date.now())
    this.#invalidate()
  }

  close(callback?: () => void) {
    if (this.#closed) {
      callback?.()
      return
    }

    this.#closed = true
    this.#watcher.close()
    this.compiler.watching = undefined
    this.compiler.hooks.watchClose.call()
    callback?.()
  }

  #defaultIgnored = (file: string) => {
    const absolutePath = path.resolve(file)
    const outputPath = path.resolve(this.compiler.options.output.path)
    return (
      absolutePath === outputPath ||
      absolutePath.startsWith(`${outputPath}${path.sep}`) ||
      /(^|[\\/])(?:\.git|node_modules)(?:[\\/]|$)/.test(absolutePath)
    )
  }

  #reportInvalid(file: string | null, modifiedTime: number) {
    if (this.#invalidReported) {
      return
    }
    this.#invalidReported = true
    this.compiler.hooks.invalid.call(file, modifiedTime)
  }

  #mergeChanges(changes: Set<string>, removals: Set<string>) {
    for (const file of changes) {
      this.#changedFiles.add(file)
      this.#removedFiles.delete(file)
    }
    for (const file of removals) {
      this.#changedFiles.delete(file)
      this.#removedFiles.add(file)
    }
  }

  #watch(files: Iterable<string>, startTime: number) {
    if (this.#closed) {
      return
    }
    this.#watcher.watch({
      files,
      startTime,
    })
  }

  #invalidate() {
    if (this.#closed) {
      return
    }
    if (this.#running) {
      this.#invalid = true
      return
    }
    void this.#go()
  }

  // 沿用 Rspack/webpack Watching 的命名：go 表示失效后启动一轮 compilation
  // （说实话这个命名很烂）
  // run 表示的是 Compiler 的一次性构建入口，而 Watching 会反复调用，所以不触发 go，所以可能是因为这个要有所区分吧。
  async #go() {
    const watcherStartTime = Date.now()
    this.#running = true
    this.#invalid = false
    this.#invalidReported = false
    // 删除文件会影响 resolve，不能当作普通内容修改处理。
    this.compiler.modifiedFiles = new Set(this.#changedFiles)
    this.compiler.removedFiles = new Set(this.#removedFiles)
    this.#changedFiles.clear()
    this.#removedFiles.clear()

    try {
      // watchRun 发生在本轮 Compilation 创建之前，描述的是 Compiler 即将开始一次 watch 构建。
      // 因此它属于 compiler hook，而不是 compilation hooks 
      // 后者只处理已经创建出的本轮编译数据
      await this.compiler.hooks.watchRun.promise(this.compiler)
      const compilation = await this.compiler.compile()

      // 当前构建期间又有变化时，丢弃这一轮结果并立即重新构建。
      if (this.#invalid) {
        return
      }

      const stats = compilation.getStats()
      await this.compiler.hooks.done.promise(stats)
      this.#handler(null, stats)
      this.compiler.hooks.afterDone.call(stats)
      this.#watch(compilation.fileDependencies, watcherStartTime)
    } catch (err) {
      const error = err instanceof Error ? err : new Error(String(err))
      this.compiler.hooks.failed.call(error)
      this.#handler(error)
    } finally {
      this.#running = false
      if (this.#invalid && !this.#closed) {
        this.#invalidate()
      }
    }
  }
}
