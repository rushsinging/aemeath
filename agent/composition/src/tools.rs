use std::sync::Arc;

use ::tools::ToolCatalogGateway;

pub fn wire_tools() -> Arc<dyn ToolCatalogGateway> {
    ::tools::wire_tools()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_tools_returns_callable_gateway() {
        let gateway = wire_tools();
        let registry = gateway.new_registry();

        assert!(!registry.contains("Read"));
    }
}
