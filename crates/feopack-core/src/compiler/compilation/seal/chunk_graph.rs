use super::super::{Chunk, Compilation};

impl Compilation {
  /**
   * 真实情况下会有一些分组策略，但是这里做简化，
   * 将所有模块放到一个 chunk 中
   */
  pub(crate) async fn create_chunk_graph(&mut self) {
    let module_ids = self.module_graph.module_ids().cloned().collect();
    let chunk = Chunk {
      id: "main".to_string(),
      module_ids,
    };
    self.chunk_graph.chunks.push(chunk);
  }
}
