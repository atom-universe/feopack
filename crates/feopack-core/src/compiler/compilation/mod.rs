mod make;
mod resolve;
mod seal;

use super::normal_module_factory::NormalModuleFactory;
use crate::loader::inline_request::InlineRequest;
use crate::loader::{JsLoaderRunner, LoaderEnforce, LoaderRegistry, LoaderRule};
use crate::module_graph::ModuleGraph;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct Output {
  pub path: String,
  pub filename: String,
}

#[derive(Debug, Clone)]
pub struct ModuleRuleOptions {
  pub test: String,
  pub use_loaders: Vec<String>,
}

// 对齐 Node-Binding 并做一些 Rust 侧的类型适配。（同时也避免循环依赖）
#[derive(Debug, Clone)]
pub struct CompilationOptions {
  pub entry: String,
  pub mode: String,
  pub context: String,
  pub output: Output,
  pub module_rules: Vec<ModuleRuleOptions>,
}

#[derive(Debug, Clone)]
pub struct Chunk {
  pub id: String,
  pub module_ids: Vec<String>,
}

#[derive(Debug, Default)]
pub struct ChunkGraph {
  pub chunks: Vec<Chunk>,
}

#[derive(Debug, Clone)]
pub struct GeneratedAsset {
  pub filename: String,
  pub source: String,
}

#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct BuildStats {
  pub(crate) invalidated_modules: usize,
  pub(crate) rebuilt_modules: usize,
  pub(crate) reused_modules: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct CodegenModule {
  // code_generation 阶段消费的临时结构：
  // 到这里已经不是“源码文件”，而是即将塞进 chunk runtime 的模块函数体。
  pub(crate) id: String,
  pub(crate) source: String,
}

#[derive(Debug, Clone)]
pub(crate) enum ResolvedPath {
  // File(PathBuf),
  File(ResolvedModule),
  External(String),
}

// PathBuf 逐渐不够用了，改造成 RsolvedModule
#[derive(Debug, Clone)]
pub(crate) struct ResolvedModule {
  // 也就是 /abs/xxx?yyy=123 这样的内容
  pub module_id: String,
  // /abs/xxx
  pub resource_path: PathBuf,
  // ?yyy=123
  pub resource_query: String,
  pub inline: InlineRequest,
}

pub struct Compilation {
  pub options: CompilationOptions,
  pub module_graph: ModuleGraph,
  pub chunk_graph: ChunkGraph,
  pub assets: Vec<GeneratedAsset>,
  pub file_dependencies: HashSet<PathBuf>,
  pub(crate) module_sources: HashMap<String, String>,
  pub(crate) file_source_cache: HashMap<PathBuf, String>,
  pub(crate) loader_registry: LoaderRegistry,
  pub(crate) normal_module_factory: NormalModuleFactory,
  pub(crate) modified_files: HashSet<PathBuf>,
  pub(crate) removed_files: HashSet<PathBuf>,
  pub(crate) build_stats: BuildStats,
  #[allow(dead_code)]
  pub(crate) js_loader_runner: Option<JsLoaderRunner>,
}

impl Compilation {
  pub fn new(options: CompilationOptions, js_loader_runner: Option<JsLoaderRunner>) -> Self {
    println!("\n\nCompilation new: {:?}\n\n", options);
    let mut loader_registry = LoaderRegistry::with_builtin_defaults();

    let user_rules = options
      .module_rules
      .iter()
      .map(|rule| LoaderRule {
        test: rule.test.clone(),
        resource_query: String::new(),
        used_loaders: rule.use_loaders.clone(),
        enforce: LoaderEnforce::Normal,
      })
      .collect();
    loader_registry.set_user_rules(user_rules);

    Self {
      options: options.clone(),
      module_graph: ModuleGraph::new(),
      chunk_graph: ChunkGraph::default(),
      assets: Vec::new(),
      file_dependencies: HashSet::new(),
      module_sources: HashMap::new(),
      file_source_cache: HashMap::new(),
      loader_registry,
      normal_module_factory: NormalModuleFactory::new(),
      modified_files: HashSet::new(),
      removed_files: HashSet::new(),
      build_stats: BuildStats::default(),
      js_loader_runner,
    }
  }

  pub(crate) fn reuse_build_state(
    &mut self,
    previous: &mut Compilation,
    modified_files: Vec<String>,
    removed_files: Vec<String>,
  ) -> Result<(), String> {
    self.module_graph = std::mem::take(&mut previous.module_graph);
    self.module_sources = std::mem::take(&mut previous.module_sources);

    // file_source_cache 只属于一次 Compilation。跨轮复用的是已经完成
    // loader 和解析的模块结果，而不是未经校验的磁盘内容。
    self.modified_files = modified_files
      .into_iter()
      .map(PathBuf::from)
      .map(|path| Self::normalize_path(&path))
      .collect::<Result<_, _>>()?;
    self.removed_files = removed_files
      .into_iter()
      .map(PathBuf::from)
      .map(|path| Self::normalize_path(&path))
      .collect::<Result<_, _>>()?;

    Ok(())
  }
}
