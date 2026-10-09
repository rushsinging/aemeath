//! 后台进程输出环形缓冲。
//!
//! 读写游标分离实现非消耗性读取：读取不推进写入侧状态，多次读取幂等，
//! 不破坏任务完成后的结果回注。全局写入游标（`total_written`）单调递增，
//! 作为增量读取的稳定坐标；容量超出时丢弃最老字节，过期游标 clamp 到
//! 当前可用窗口。文本读取使用 lossy 渲染，容忍多字节字符边界切割。

#[derive(Debug, Clone)]
pub struct OutputRingBuffer {
    capacity_bytes: usize,
    buffer: Vec<u8>,
    /// 累计写入字节数（全局单调游标）。
    total_written: u64,
    /// `buffer[0]` 对应的全局游标位置。
    buffer_start_cursor: u64,
}

impl OutputRingBuffer {
    pub fn new(capacity_bytes: usize) -> Self {
        Self {
            capacity_bytes,
            buffer: Vec::new(),
            total_written: 0,
            buffer_start_cursor: 0,
        }
    }

    /// 追加输出字节；超出容量时丢弃最老字节。
    pub fn append(&mut self, chunk: &[u8]) {
        if chunk.len() >= self.capacity_bytes {
            let tail = chunk
                .get(chunk.len() - self.capacity_bytes..)
                .expect("chunk.len() >= capacity 分支内尾部区间必然合法");
            self.buffer = tail.to_vec();
            self.total_written += chunk.len() as u64;
            self.buffer_start_cursor = self.total_written - self.capacity_bytes as u64;
            return;
        }
        self.buffer.extend_from_slice(chunk);
        self.total_written += chunk.len() as u64;
        let overflow = if self.buffer.len() > self.capacity_bytes {
            self.buffer.len() - self.capacity_bytes
        } else {
            0
        };
        if overflow > 0 {
            self.buffer.drain(..overflow);
            self.buffer_start_cursor += overflow as u64;
        }
    }

    /// 尾部视图（非消耗性）：返回最近 `max_bytes` 字节与读后游标。
    pub fn read_tail_bytes(&self, max_bytes: usize) -> (Vec<u8>, u64) {
        let start = self.buffer.len().saturating_sub(max_bytes);
        let bytes = self
            .buffer
            .get(start..)
            .expect("saturating_sub 起点必然不越界")
            .to_vec();
        let cursor = self.buffer_start_cursor + start as u64 + bytes.len() as u64;
        (bytes, cursor)
    }

    /// 从游标起的增量读取（非消耗性）：仅返回 `cursor` 之后的新字节；
    /// 过期游标（数据已被覆盖）clamp 到当前可用窗口起点。
    pub fn read_from_bytes(&self, cursor: u64, max_bytes: usize) -> (Vec<u8>, u64) {
        let effective_start = cursor.max(self.buffer_start_cursor);
        let end = effective_start
            .saturating_add(max_bytes as u64)
            .min(self.total_written());
        let buffer_offset_start = (effective_start - self.buffer_start_cursor) as usize;
        let buffer_offset_end = (end - self.buffer_start_cursor) as usize;
        let bytes = self
            .buffer
            .get(buffer_offset_start..buffer_offset_end)
            .expect("游标区间已由 buffer_start_cursor 与 total_written clamp 保证合法")
            .to_vec();
        (bytes, end)
    }

    /// 尾部文本视图（lossy 渲染）。
    pub fn read_tail_text(&self, max_bytes: usize) -> (String, u64) {
        let (bytes, cursor) = self.read_tail_bytes(max_bytes);
        (String::from_utf8_lossy(&bytes).into_owned(), cursor)
    }

    /// 增量文本视图（lossy 渲染）。
    pub fn read_from_text(&self, cursor: u64, max_bytes: usize) -> (String, u64) {
        let (bytes, end) = self.read_from_bytes(cursor, max_bytes);
        (String::from_utf8_lossy(&bytes).into_owned(), end)
    }

    /// 累计写入字节数（全局单调游标）。
    pub fn total_written(&self) -> u64 {
        self.total_written
    }

    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }
}

#[cfg(test)]
#[path = "output_ring_buffer_tests.rs"]
mod tests;
