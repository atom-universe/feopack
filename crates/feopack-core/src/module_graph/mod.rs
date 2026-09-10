pub mod module;
pub use module::Module;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;

#[derive(Debug, Default)]
pub struct ModuleGraph {
  modules: HashMap<String, Module>,
  // 一个物理文件可以通过 query 或 inline loader 产生多个逻辑模块。
  modules_by_resource: HashMap<PathBuf, HashSet<String>>,
  // 被引用模块的 ID -> 引用它的模块 ID 集合。
  incoming_modules: HashMap<String, HashSet<String>>,
}

impl ModuleGraph {
  pub fn new() -> Self {
    Self::default()
  }

  pub fn add_module(&mut self, module: Module) {
    let module_id = module.id.clone();
    self.remove_module(&module_id);

    self
      .modules_by_resource
      .entry(module.resource_path.clone())
      .or_default()
      .insert(module_id.clone());

    for dependency_id in &module.local_dependency_ids {
      self
        .incoming_modules
        .entry(dependency_id.clone())
        .or_default()
        .insert(module_id.clone());
    }

    self.modules.insert(module_id, module);
  }

  pub fn has_module(&self, id: &str) -> bool {
    self.modules.contains_key(id)
  }

  pub fn get_module(&self, id: &str) -> Option<&Module> {
    self.modules.get(id)
  }

  pub fn module_ids(&self) -> impl Iterator<Item = &String> {
    self.modules.keys()
  }

  pub fn len(&self) -> usize {
    self.modules.len()
  }

  pub fn is_empty(&self) -> bool {
    self.modules.is_empty()
  }

  pub fn resource_paths(&self) -> impl Iterator<Item = &PathBuf> {
    self.modules_by_resource.keys()
  }

  pub fn module_ids_for_resources<'a>(
    &'a self,
    resources: impl IntoIterator<Item = &'a PathBuf>,
  ) -> HashSet<String> {
    resources
      .into_iter()
      .filter_map(|resource| self.modules_by_resource.get(resource))
      .flatten()
      .cloned()
      .collect()
  }

  pub fn affected_modules(&self, invalidated: HashSet<String>) -> HashSet<String> {
    let mut affected = invalidated;
    let mut queue = affected.iter().cloned().collect::<VecDeque<_>>();

    while let Some(module_id) = queue.pop_front() {
      let Some(importers) = self.incoming_modules.get(&module_id) else {
        continue;
      };

      for importer in importers {
        if affected.insert(importer.clone()) {
          queue.push_back(importer.clone());
        }
      }
    }

    affected
  }

  pub fn remove_module(&mut self, module_id: &str) -> Option<Module> {
    let module = self.modules.remove(module_id)?;

    if let Some(module_ids) = self.modules_by_resource.get_mut(&module.resource_path) {
      module_ids.remove(module_id);
      if module_ids.is_empty() {
        self.modules_by_resource.remove(&module.resource_path);
      }
    }

    for dependency_id in &module.local_dependency_ids {
      if let Some(importers) = self.incoming_modules.get_mut(dependency_id) {
        importers.remove(module_id);
        if importers.is_empty() {
          self.incoming_modules.remove(dependency_id);
        }
      }
    }
    Some(module)
  }

  pub fn retain_modules(&mut self, retained: &HashSet<String>) {
    let removed = self
      .modules
      .keys()
      .filter(|module_id| !retained.contains(*module_id))
      .cloned()
      .collect::<Vec<_>>();

    for module_id in removed {
      self.remove_module(&module_id);
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn module(id: &str, resource: &str, dependencies: &[&str]) -> Module {
    Module::new(
      id.to_string(),
      PathBuf::from(resource),
      Vec::new(),
      dependencies.iter().map(|id| id.to_string()).collect(),
    )
  }

  #[test]
  fn one_resource_can_invalidate_multiple_logical_modules() {
    let mut graph = ModuleGraph::new();
    graph.add_module(module("/app.view", "/app.view", &[]));
    graph.add_module(module("/app.view?type=script", "/app.view", &[]));

    let resource = PathBuf::from("/app.view");
    let invalidated = graph.module_ids_for_resources([&resource]);

    assert_eq!(invalidated.len(), 2);
    assert!(invalidated.contains("/app.view"));
    assert!(invalidated.contains("/app.view?type=script"));
  }

  #[test]
  fn invalidation_propagates_through_importers() {
    let mut graph = ModuleGraph::new();
    graph.add_module(module("entry", "/entry.js", &["feature"]));
    graph.add_module(module("feature", "/feature.js", &["leaf"]));
    graph.add_module(module("leaf", "/leaf.js", &[]));
    graph.add_module(module("unrelated", "/unrelated.js", &[]));

    let affected = graph.affected_modules(HashSet::from(["leaf".to_string()]));

    assert_eq!(affected.len(), 3);
    assert!(affected.contains("leaf"));
    assert!(affected.contains("feature"));
    assert!(affected.contains("entry"));
    assert!(!affected.contains("unrelated"));
  }

  #[test]
  fn replacing_a_module_updates_reverse_edges() {
    let mut graph = ModuleGraph::new();
    graph.add_module(module("entry", "/entry.js", &["old"]));
    graph.add_module(module("entry", "/entry.js", &["new"]));

    let affected_by_old = graph.affected_modules(HashSet::from(["old".to_string()]));
    let affected_by_new = graph.affected_modules(HashSet::from(["new".to_string()]));

    assert!(!affected_by_old.contains("entry"));
    assert!(affected_by_new.contains("entry"));
  }

  #[test]
  fn replacing_a_target_keeps_its_importers() {
    let mut graph = ModuleGraph::new();
    graph.add_module(module("entry", "/entry.js", &["leaf"]));
    graph.add_module(module("leaf", "/leaf.js", &[]));
    graph.add_module(module("leaf", "/leaf.js", &[]));

    let affected = graph.affected_modules(HashSet::from(["leaf".to_string()]));

    assert!(affected.contains("entry"));
  }
}
