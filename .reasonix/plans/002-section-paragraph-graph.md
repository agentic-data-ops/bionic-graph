# Plan 002: 文档章节/段落结构入图

## 目标

将文档的章节结构和段落内容也存入知识图谱，建立「实体 ↔ 章节 ↔ 段落」的完整关联。

## 当前状态

提取管道 `extract_sections_core` 目前只把 LLM 提取的实体插入图库，丢弃了文档结构信息。

## 设计

### 新增顶点类型

| 标签 | 说明 | 属性 |
|------|------|------|
| `section` | 文档章节（heading） | `heading`, `depth`, `heading_chain`, `doc_index` |
| `paragraph` | 章节内的一段文字 | `content`, `paragraph_index` |

### 新增边类型

| 标签 | 源 | 目标 | 说明 |
|------|----|------|------|
| `has_subsection` | section | section | 父章节 → 子章节 |
| `belongs_to` | paragraph | section | 段落归属于哪个章节 |
| `mentioned_in_section` | entity | section | 实体出现在哪个章节 |
| `mentioned_in_paragraph` | entity | paragraph | 实体出现在哪个段落 |

### 段落拆分策略

Section 的 `content` 按空行分割为段落：
```
"第一段文字\n\n第二段文字\n\n第三段文字" 
→ ["第一段文字", "第二段文字", "第三段文字"]
```

### 实现步骤

| 步骤 | 内容 | 文件 |
|------|------|------|
| 1 | 新增 `insert_section_paragraphs` 函数：将章节拆为段落并插入图库 | `pipeline.rs` |
| 2 | 新增 `link_entities_to_section` 函数：建立实体↔章节的关系边 | `pipeline.rs` |
| 3 | 在 `extract_sections_core` 主循环中调用上述函数 | `pipeline.rs` |
| 4 | 更新 `ExtractionStats` 统计章节/段落数 | `pipeline.rs` |
| 5 | `cargo test` 验证 | - |
