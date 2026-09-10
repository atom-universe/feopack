pub mod compilation;
mod emit;
mod hooks;
mod normal_module_factory;
mod plugin;

use crate::loader::JsLoaderRunner;
use compilation::{Compilation, CompilationOptions};
use hooks::CompilerHooks;
use plugin::PluginDriver;
pub use plugin::{apply_builtin_plugin, Plugin};
use std::future::Future;
use std::pin::Pin;
use std::result::Result;
use std::sync::Arc;

// Rspack binding 为每个 compiler hook 提供独立 adapter；mini 版统一用一个事件回调桥接。
pub struct JsCompilerHookEvent {
  pub name: &'static str,
  pub filename: Option<String>,
  pub target_path: Option<String>,
}

pub type JsHooksAdapterFuture = Pin<Box<dyn Future<Output = Result<(), String>> + Send>>;
pub type JsHooksAdapter = Arc<dyn Fn(JsCompilerHookEvent) -> JsHooksAdapterFuture + Send + Sync>;

// 主流程与 Rspack 保持相同的职责划分：Compiler 负责阶段编排和触发 hooks，
// Compilation 负责构建 module graph、seal 和生成 assets 等具体工作。
pub struct Compiler {
  compilation: Compilation,
  options: CompilationOptions,
  js_loader_runner: Option<JsLoaderRunner>,
  js_hooks_adapter: Option<JsHooksAdapter>,
  plugin_driver: PluginDriver,
}

impl Compiler {
  pub fn new(options: CompilationOptions) -> Self {
    Self {
      compilation: Compilation::new(options.clone(), None),
      options,
      js_loader_runner: None,
      js_hooks_adapter: None,
      plugin_driver: PluginDriver::default(),
    }
  }

  pub fn set_js_loader_runner(&mut self, runner: Option<JsLoaderRunner>) {
    self.js_loader_runner = runner;
  }

  pub fn set_js_hooks_adapter(&mut self, adapter: Option<JsHooksAdapter>) {
    self.js_hooks_adapter = adapter;
  }

  pub fn file_dependencies(&self) -> Vec<String> {
    let mut dependencies = self
      .compilation
      .file_dependencies
      .iter()
      .map(|path| path.to_string_lossy().into_owned())
      .collect::<Vec<_>>();
    dependencies.sort();
    dependencies
  }

  pub(crate) fn hooks(&self) -> &CompilerHooks {
    self.plugin_driver.compiler_hooks()
  }

  // 为什么要有 hooks 了还要 hooks_mut呢
  // 主要是照顾 rust 侧的 mut 机制，在编译期间就确定权限
  #[allow(dead_code)]
  pub(crate) fn hooks_mut(&mut self) -> &mut CompilerHooks {
    self.plugin_driver.compiler_hooks_mut()
  }

  pub async fn build(&mut self) -> Result<(), String> {
    self.compilation = Compilation::new(self.options.clone(), self.js_loader_runner.clone());
    self.compile().await?;
    self.compile_done().await?;
    Ok(())
  }

  pub async fn rebuild(
    &mut self,
    modified_files: Vec<String>,
    removed_files: Vec<String>,
  ) -> Result<(), String> {
    let mut next_compilation =
      Compilation::new(self.options.clone(), self.js_loader_runner.clone());
    next_compilation.reuse_build_state(&mut self.compilation, modified_files, removed_files)?;
    self.compilation = next_compilation;

    self.compile().await?;
    self.compile_done().await?;
    Ok(())
  }

  async fn compile(&mut self) -> Result<(), String> {
    self.build_module_graph().await?;
    self.compilation.seal().await?;
    Ok(())
  }

  async fn compile_done(&mut self) -> Result<(), String> {
    let should_emit = self.hooks().should_emit.call(&())?.unwrap_or(true);
    if !should_emit {
      println!("[rust compiler lifecycle] should_emit returned false, skip emit");
      return Ok(());
    }
    self.emit_assets().await?;
    Ok(())
  }

  async fn build_module_graph(&mut self) -> Result<(), String> {
    self
      .call_js_compiler_hook("thisCompilation", None, None)
      .await?;
    self
      .call_js_compiler_hook("compilation", None, None)
      .await?;
    self.hooks().make.call(&())?;
    self.call_js_compiler_hook("make", None, None).await?;
    self.compilation.build_module_graph().await
  }

  async fn call_js_compiler_hook(
    &self,
    name: &'static str,
    filename: Option<String>,
    target_path: Option<String>,
  ) -> Result<(), String> {
    let Some(adapter) = &self.js_hooks_adapter else {
      return Ok(());
    };
    adapter(JsCompilerHookEvent {
      name,
      filename,
      target_path,
    })
    .await
  }
}

#[cfg(test)]
mod tests {
  use super::compilation::{CompilationOptions, Output};
  use super::hooks::AssetEmittedContext;
  use super::Compiler;
  use std::path::Path;
  use std::sync::{Arc, Mutex};
  use std::time::{SystemTime, UNIX_EPOCH};

  fn test_compiler() -> Compiler {
    Compiler::new(CompilationOptions {
      entry: "src/index.js".to_string(),
      mode: "development".to_string(),
      context: "/tmp/feopack-test".to_string(),
      output: Output {
        path: "/tmp/feopack-test/dist".to_string(),
        filename: "main.js".to_string(),
      },
      module_rules: Vec::new(),
    })
  }

  #[test]
  fn compiler_hook_calls_registered_tap() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let mut compiler = test_compiler();

    let make_calls = Arc::clone(&calls);
    compiler.hooks_mut().make.tap("test-make", move |_| {
      make_calls.lock().expect("lock calls").push("make");
      Ok(())
    });

    compiler.hooks().make.call(&()).expect("make should pass");

    assert_eq!(*calls.lock().expect("lock calls"), vec!["make"]);
  }

  #[test]
  fn asset_emitted_hook_receives_context() {
    let seen = Arc::new(Mutex::new(None));
    let mut compiler = test_compiler();

    let seen_context = Arc::clone(&seen);
    compiler
      .hooks_mut()
      .asset_emitted
      .tap("test-asset-emitted", move |ctx| {
        *seen_context.lock().expect("lock seen") =
          Some((ctx.filename.clone(), ctx.target_path.clone()));
        Ok(())
      });

    compiler
      .hooks()
      .asset_emitted
      .call(&AssetEmittedContext {
        filename: "main.js".to_string(),
        target_path: Path::new("/tmp/feopack-test/dist/main.js").to_path_buf(),
      })
      .expect("asset_emitted should pass");

    assert_eq!(
      *seen.lock().expect("lock seen"),
      Some((
        "main.js".to_string(),
        Path::new("/tmp/feopack-test/dist/main.js").to_path_buf()
      ))
    );
  }

  #[test]
  fn should_emit_bail_hook_can_skip_emit() {
    let mut compiler = test_compiler();

    compiler
      .hooks_mut()
      .should_emit
      .tap("skip-emit", |_| Ok(Some(false)));

    assert_eq!(
      compiler
        .hooks()
        .should_emit
        .call(&())
        .expect("should_emit should pass"),
      Some(false)
    );
  }

  #[test]
  fn rebuild_reuses_an_unaffected_branch() {
    let suffix = SystemTime::now()
      .duration_since(UNIX_EPOCH)
      .expect("clock should be after unix epoch")
      .as_nanos();
    let root = std::env::temp_dir().join(format!(
      "feopack-incremental-{}-{suffix}",
      std::process::id()
    ));
    let src = root.join("src");
    std::fs::create_dir_all(&src).expect("create fixture directory");
    std::fs::write(
      src.join("index.js"),
      "import { feature } from './feature.js'; import { stable } from './stable.js'; console.log(feature, stable);",
    )
    .expect("write entry");
    std::fs::write(
      src.join("feature.js"),
      "import { leaf } from './leaf.js'; export const feature = leaf;",
    )
    .expect("write feature");
    std::fs::write(src.join("leaf.js"), "export const leaf = 'v1';").expect("write leaf");
    std::fs::write(src.join("stable.js"), "export const stable = 'stable';").expect("write stable");

    let mut compiler = Compiler::new(CompilationOptions {
      entry: "src/index.js".to_string(),
      mode: "development".to_string(),
      context: root.to_string_lossy().into_owned(),
      output: Output {
        path: root.join("dist").to_string_lossy().into_owned(),
        filename: "main.js".to_string(),
      },
      module_rules: Vec::new(),
    });
    let runtime = tokio::runtime::Builder::new_current_thread()
      .enable_all()
      .build()
      .expect("create tokio runtime");

    runtime.block_on(compiler.build()).expect("initial build");
    std::fs::write(src.join("leaf.js"), "export const leaf = 'v2';").expect("update leaf");
    runtime
      .block_on(compiler.rebuild(
        vec![src.join("leaf.js").to_string_lossy().into_owned()],
        Vec::new(),
      ))
      .expect("incremental rebuild");

    assert_eq!(compiler.compilation.build_stats.invalidated_modules, 3);
    assert_eq!(compiler.compilation.build_stats.rebuilt_modules, 3);
    assert_eq!(compiler.compilation.build_stats.reused_modules, 1);
    let bundle = std::fs::read_to_string(root.join("dist/main.js")).expect("read bundle");
    assert!(bundle.contains("v2"));
    assert!(bundle.contains("stable"));

    std::fs::write(
      src.join("feature.js"),
      "export const feature = 'without leaf';",
    )
    .expect("remove leaf import");
    runtime
      .block_on(compiler.rebuild(
        vec![src.join("feature.js").to_string_lossy().into_owned()],
        Vec::new(),
      ))
      .expect("rebuild after dependency removal");

    let leaf_id = src.join("leaf.js").to_string_lossy().into_owned();
    assert!(!compiler.compilation.module_graph.has_module(&leaf_id));
    let bundle = std::fs::read_to_string(root.join("dist/main.js")).expect("read rebuilt bundle");
    assert!(!bundle.contains("v2"));

    std::fs::remove_dir_all(root).expect("remove fixture directory");
  }
}
