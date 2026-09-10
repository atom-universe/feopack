use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct Module {
  pub id: String,
  pub resource_path: PathBuf,
  // 保留源码中的 request，供代码生成和诊断使用。
  pub dependencies: Vec<String>,
  // 图遍历需要解析后的模块 ID；它不能由 request 字符串代替。
  pub local_dependency_ids: Vec<String>,
}

impl Module {
  pub fn new(
    id: String,
    resource_path: PathBuf,
    dependencies: Vec<String>,
    local_dependency_ids: Vec<String>,
  ) -> Self {
    Self {
      id,
      resource_path,
      dependencies,
      local_dependency_ids,
    }
  }
}
