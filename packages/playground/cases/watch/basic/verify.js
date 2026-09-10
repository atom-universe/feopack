const assert = require('node:assert')
const fs = require('node:fs')
const path = require('node:path')
const feopack = require('@feopack/core').default

// pnpm test watch/basic 会等待这个异步 case 断言完成；它验证完整 watch/rebuild 流程，
// 属于 playground 集成测试，不是针对单个函数的单元测试。
module.exports = function verify(config) {
  const sourcePath = path.join(config.context, 'src', 'message.js')
  const bundlePath = path.join(config.output.path, config.output.filename)
  const logPath = path.join(config.output.path, 'watch-hooks.log')
  const originalSource = fs.readFileSync(sourcePath, 'utf8')

  fs.rmSync(logPath, { force: true })

  return new Promise((resolve, reject) => {
    let buildCount = 0
    let finished = false
    let watching

    const finish = (error) => {
      if (finished) return
      finished = true
      clearTimeout(timeout)
      watching?.close(() => {
        fs.writeFileSync(sourcePath, originalSource)
        if (error) reject(error)
        else resolve()
      })
    }

    const timeout = setTimeout(() => {
      finish(new Error('watch rebuild timed out'))
    }, 15_000)

    const compiler = feopack(config)
    watching = compiler.watch({ aggregateTimeout: 20 }, (error) => {
      if (error) {
        finish(error)
        return
      }

      buildCount += 1
      const bundle = fs.readFileSync(bundlePath, 'utf8')

      if (buildCount === 1) {
        assert.match(bundle, /watch-v1/)
        assert.match(bundle, /unchanged/)
        fs.writeFileSync(sourcePath, "export const message = 'watch-v2'\n")
        return
      }

      try {
        assert.match(bundle, /watch-v2/)
        assert.match(bundle, /unchanged/)
        assert.ok(
          compiler.modifiedFiles?.has(sourcePath),
          `modified files: ${[...(compiler.modifiedFiles ?? [])].join(', ')}`,
        )
        finish()
      } catch (assertionError) {
        finish(assertionError)
      }
    })
  }).then(() => {
    const events = fs.readFileSync(logPath, 'utf8').trim().split(/\r?\n/)
    assert.deepStrictEqual(events, ['watchRun', 'invalid', 'watchRun', 'watchClose'])
  })
}
