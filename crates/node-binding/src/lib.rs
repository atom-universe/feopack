#![deny(clippy::all)]

mod js_hooks_adapter;
mod js_loader;
mod js_loader_types;

use feopack_binding::options::RawOptions;
use js_hooks_adapter::{create_js_hooks_adapter, JsCompilerHookEventInput, JsHooksAdapterTsFn};
use js_loader::{create_js_loader_runner, JsLoaderRunnerTsFn};
use js_loader_types::{JsLoaderContextInput, JsLoaderResultOutput};
use napi::bindgen_prelude::*;
use napi::{Error, Result};
use napi_derive::napi;
use std::sync::Arc;

use feopack_core::*;

#[napi]
pub struct Rspack {
  compiler: Box<Compiler>,
  js_loader_runner: Option<Arc<JsLoaderRunnerTsFn>>,
  js_hooks_adapter: Option<Arc<JsHooksAdapterTsFn>>,
}

#[napi]
impl Rspack {
  #[napi(constructor)]
  pub fn new(
    options: RawOptions,
    // 这里有一个 FFI 参数，从 js 那那边拿到 js loader runner 的句柄
    #[napi(ts_arg_type = "(ctx: JsLoaderContextInput) => Promise<JsLoaderResultOutput>")]
    js_loader_runner: Option<Function<JsLoaderContextInput, Promise<JsLoaderResultOutput>>>,
    #[napi(ts_arg_type = "(event: JsCompilerHookEventInput) => Promise<void>")]
    js_hooks_adapter: Option<Function<JsCompilerHookEventInput, Promise<()>>>,
  ) -> Result<Self> {
    // 读 js 那边 注册的 loader
    let module_rules = options
      .module
      .map(|module| {
        module
          .rules
          .into_iter()
          .map(|rule| ModuleRuleOptions {
            test: rule.test,
            use_loaders: rule.use_loaders,
          })
          .collect::<Vec<_>>()
      })
      .unwrap_or_default();

    let compilation_options = CompilationOptions {
      mode: options.mode,
      entry: options.entry,
      context: options.context,
      output: Output {
        path: options.output.path,
        filename: options.output.filename,
      },
      module_rules,
    };

    let mut compiler = Compiler::new(compilation_options);

    for plugin_name in options.rust_plugins.unwrap_or_default() {
      apply_builtin_plugin(&plugin_name, &mut compiler).map_err(Error::from_reason)?;
    }

    let js_loader_runner = js_loader_runner
      .map(|runner_fn| -> Result<Arc<JsLoaderRunnerTsFn>> {
        let ts_fn: JsLoaderRunnerTsFn = runner_fn
          .build_threadsafe_function::<JsLoaderContextInput>()
          .max_queue_size::<0>()
          .build()?;
        Ok(Arc::new(ts_fn))
      })
      .transpose()?;

    let js_hooks_adapter = js_hooks_adapter
      .map(|adapter_fn| -> Result<Arc<JsHooksAdapterTsFn>> {
        let ts_fn: JsHooksAdapterTsFn = adapter_fn
          .build_threadsafe_function::<JsCompilerHookEventInput>()
          .max_queue_size::<0>()
          .build()?;
        Ok(Arc::new(ts_fn))
      })
      .transpose()?;

    Ok(Self {
      compiler: Box::new(compiler),
      js_loader_runner,
      js_hooks_adapter,
    })
  }

  #[napi]
  pub async unsafe fn build(&mut self) -> Result<()> {
    if let Some(runner_ref) = self.js_loader_runner.as_ref() {
      let runner = create_js_loader_runner(Arc::clone(runner_ref));
      self.compiler.set_js_loader_runner(Some(runner));
    }

    if let Some(adapter_ref) = self.js_hooks_adapter.as_ref() {
      let adapter = create_js_hooks_adapter(Arc::clone(adapter_ref));
      self.compiler.set_js_hooks_adapter(Some(adapter));
    }

    self
      .compiler
      .build()
      .await
      .map_err(|e| Error::from_reason(e))
  }

  #[napi]
  pub async unsafe fn rebuild(
    &mut self,
    modified_files: Vec<String>,
    removed_files: Vec<String>,
  ) -> Result<()> {
    if let Some(runner_ref) = self.js_loader_runner.as_ref() {
      let runner = create_js_loader_runner(Arc::clone(runner_ref));
      self.compiler.set_js_loader_runner(Some(runner));
    }

    if let Some(adapter_ref) = self.js_hooks_adapter.as_ref() {
      let adapter = create_js_hooks_adapter(Arc::clone(adapter_ref));
      self.compiler.set_js_hooks_adapter(Some(adapter));
    }

    self
      .compiler
      .rebuild(modified_files, removed_files)
      .await
      .map_err(Error::from_reason)
  }

  #[napi]
  pub fn get_file_dependencies(&self) -> Vec<String> {
    self.compiler.file_dependencies()
  }
}
