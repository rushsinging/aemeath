/// 从 tar.gz 归档中提取 `aemeath` 二进制文件内容。
///
/// 归档结构：`aemeath-{version}-{target}/aemeath`
pub(super) fn extract_binary_from_tar_gz(data: &[u8]) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let gz = flate2::read::GzDecoder::new(data);
    let mut archive = tar::Archive::new(gz);

    for entry in archive
        .entries()
        .map_err(|e| format!("读取 tar 条目失败: {e}"))?
    {
        let mut entry = entry.map_err(|e| format!("解析 tar 条目失败: {e}"))?;
        let path = entry.path().map_err(|e| format!("读取条目路径失败: {e}"))?;
        // 匹配归档内任意层级的 `aemeath` 文件
        if path.file_name().is_some_and(|f| f == "aemeath") {
            let mut buf = Vec::new();
            entry
                .read_to_end(&mut buf)
                .map_err(|e| format!("读取二进制内容失败: {e}"))?;
            return Ok(buf);
        }
    }

    Err("归档中未找到 aemeath 二进制文件".into())
}
