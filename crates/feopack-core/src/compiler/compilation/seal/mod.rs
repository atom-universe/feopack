mod chunk_graph;
mod code_generation;

use super::Compilation;

impl Compilation {
  pub async fn seal(&mut self) -> Result<(), String> {
    // module graph 到这里以后先视为稳定，seal 负责把它组织成 chunk，并继续生成 assets。
    println!(
      "[Rust Seal] 将包含 {} 个模块的 ModuleGraph 转换为 ChunkGraph",
      self.module_graph.len()
    );
    self.create_chunk_graph().await;
    self.code_generation().await?;
    Ok(())
  }
}
