use super::super::normal_module_factory::NormalModuleCreateData;
use super::{Compilation, ResolvedPath};
use crate::loader::runner::LoaderRunner;
use crate::loader::LoaderContext;
use crate::module_graph::Module;
use crate::swc_compiler::SwcCompiler;
use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};
use swc_ecma_ast::{ModuleDecl, ModuleItem, Program};
use tokio::fs::read_to_string;

impl Compilation {
  /*
   * 这里 rspack 实际情况是把数据结构转为 EntryDependency, 然后通过 ModuleFactory 创建 Module
   * 另外还会开一个 Task Loop 做并行调度, 这里就用 BFS 大致模拟
   * TODO: 等我完全处理好了基本的打包流程后，再回头看下 rspack 是怎么做的
   */
  pub(crate) async fn build_module_graph(&mut self) -> Result<(), String> {
    let invalidated = self
      .module_graph
      .module_ids_for_resources(self.modified_files.iter().chain(self.removed_files.iter()));
    let affected = self.module_graph.affected_modules(invalidated);
    self.build_stats.invalidated_modules = affected.len();

    for module_id in affected {
      self.module_graph.remove_module(&module_id);
      self.module_sources.remove(&module_id);
    }

    let context = Path::new(&self.options.context);
    println!("========\ncontext: {:?}\n", context);
    let entry_module_path = context.join(&self.options.entry);
    // 入口没有查询参数或内联 loader，它的模块 ID 暂时等于规范化路径。
    let entry_module_id = Self::create_module_id(&entry_module_path)?;
    println!("========\nentry_path: {:?}\n", entry_module_id);

    let mut module_id_queue: VecDeque<String> = VecDeque::new();
    // 朴素剪枝：当前只处理静态 import，所以 queue + visited 已经能表达最小 module graph 构建。
    let mut visited: HashSet<String> = HashSet::new();

    module_id_queue.push_back(entry_module_id);

    while let Some(module_id) = module_id_queue.pop_front() {
      if visited.contains(&module_id) {
        continue;
      }
      visited.insert(module_id.clone());

      let dependency_ids = if let Some(module) = self.module_graph.get_module(&module_id) {
        self.build_stats.reused_modules += 1;
        module.local_dependency_ids.clone()
      } else {
        self.build_stats.rebuilt_modules += 1;
        self.build_module(module_id).await?
      };

      for dependency_id in dependency_ids {
        module_id_queue.push_back(dependency_id);
      }
    }

    // 从入口重新走一遍可达性，顺便清理依赖删除后留下的孤立模块。
    self.module_graph.retain_modules(&visited);
    self
      .module_sources
      .retain(|module_id, _| self.module_graph.has_module(module_id));
    self.file_dependencies = self.module_graph.resource_paths().cloned().collect();

    println!(
      "[Rust Make] 失效模块：{}，重新构建：{}，复用：{}",
      self.build_stats.invalidated_modules,
      self.build_stats.rebuilt_modules,
      self.build_stats.reused_modules
    );

    Ok(())
  }

  async fn build_module(&mut self, module_id: String) -> Result<Vec<String>, String> {
    let context_path = self.options.context.clone();
    let context = Path::new(&context_path);
    let create_data = self
      .normal_module_factory
      .create(module_id, &self.loader_registry);
    let module_path = create_data.resource_path.clone();
    self
      .file_dependencies
      .insert(Self::normalize_path(&module_path)?);
    let source = self.load_module_source(&create_data).await?;

    // 这里每次 build module 都临时创建 SwcCompiler。
    // 当初是为了快速绕开 source_map/引用生命周期问题；坏处是暂时共享不了 sourcemap。
    let compiler = SwcCompiler::new();
    let ast = compiler.parse_js(module_path.clone(), source.clone())?;

    // 字面意思，也就是依赖的模块
    // external 和 internal 的区别就是，后者进入 module graph，会参与打包，而前者不会——知道这个原理，实现 external 就方便了
    let mut dependency_requests = Vec::new();
    let mut local_dependency_ids = Vec::new();

    let Program::Module(module) = ast else {
      return Err("不支持 Script 模式".into());
    };

    let module_dir = module_path
      .parent()
      .ok_or_else(|| format!("无法获取模块目录: {:?}", module_path))?;

    for item in module.body {
      if let ModuleItem::ModuleDecl(ModuleDecl::Import(import)) = item {
        if let Some(dep) = import.src.value.as_str() {
          let dep = dep.to_string();
          dependency_requests.push(dep.clone());

          // external 依赖只记录在 dependencies 中，不进入本地构建队列。
          if let ResolvedPath::File(resolved_module) =
            Self::resolve_path(&dep, module_dir, context)?
          {
            local_dependency_ids.push(resolved_module.module_id);
          }
        }
      }
    }

    // loader 已经在 make 阶段执行过，这里保存 transformed source。
    // code_generation 后续只消费这个 build result，不再重新读文件或重新跑 loader。
    self
      .module_sources
      .insert(create_data.module_id.clone(), source);

    let module = Module::new(
      create_data.module_id,
      Self::normalize_path(&module_path)?,
      dependency_requests,
      local_dependency_ids.clone(),
    );
    self.module_graph.add_module(module);

    Ok(local_dependency_ids)
  }

  async fn load_module_source(
    &mut self,
    create_data: &NormalModuleCreateData,
  ) -> Result<String, String> {
    let module_path = &create_data.resource_path;
    let query = &create_data.resource_query;
    let loader_chain = &create_data.loaders;
    let pitch_context = LoaderContext {
      resource_path: module_path.clone(),
      resource_query: query.to_string(),
      source: String::new(),
    };

    let normal_context = LoaderContext {
      resource_path: module_path.clone(),
      resource_query: query.to_string(),
      source: String::new(),
    };

    // 因为 pitch 阶段可以 return 短路掉，所以得记录下结束位置
    // 不过一定要注意啊，触发 pitch 短路的那个 loader 本身，不会参与 normal 阶段的运行
    let pitch_runner = LoaderRunner::new(
      &self.loader_registry,
      self.js_loader_runner.as_ref(),
      &self.options.context,
    );
    let (source, normal_start_index) = pitch_runner
      .run_pitch_chain(&pitch_context, loader_chain)
      .await?;

    let source = if let Some(source) = source {
      source
    } else {
      self.read_resource_file(module_path).await?
    };

    let normal_runner = LoaderRunner::new(
      &self.loader_registry,
      self.js_loader_runner.as_ref(),
      &self.options.context,
    );
    normal_runner
      .run_normal_chain(normal_context, loader_chain, normal_start_index, source)
      .await
  }

  /// 读磁盘原文，同一次 compilation 内按 resource_path 去重。
  async fn read_resource_file(&mut self, module_path: &PathBuf) -> Result<String, String> {
    let cache_key = Self::normalize_path(module_path)?;

    if let Some(cached) = self.file_source_cache.get(&cache_key) {
      return Ok(cached.clone());
    }

    let content = read_to_string(&cache_key)
      .await
      .map_err(|e| format!("读取模块文件失败 {:?}: {}", cache_key, e))?;

    self.file_source_cache.insert(cache_key, content.clone());
    Ok(content)
  }
}
