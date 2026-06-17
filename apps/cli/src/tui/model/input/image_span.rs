use sdk::ClipboardImageView;

/// 输入文档中图片占位符的区间记录。
///
/// `placeholder`（如 `[Image #1]`）出现在 buffer 的 `[start, end)` 区间，
/// `index` 是插入时分配的序号（固定不重排，删除后留空洞），
/// `image` 持有实际的图片数据。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImageSpan {
    pub index: usize,
    pub image: ClipboardImageView,
    pub start: usize,
    pub end: usize,
}

impl ImageSpan {
    pub fn new(index: usize, image: ClipboardImageView, start: usize, end: usize) -> Self {
        Self {
            index,
            image,
            start,
            end,
        }
    }

    /// 该 span 在 buffer 中占用的占位文本
    pub fn placeholder(&self) -> String {
        format!("[Image #{}]", self.index)
    }
}
