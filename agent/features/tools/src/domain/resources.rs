use crate::domain::ToolListProvider;

pub trait CatalogQuery: ToolListProvider {}
impl<T: ToolListProvider + ?Sized> CatalogQuery for T {}
