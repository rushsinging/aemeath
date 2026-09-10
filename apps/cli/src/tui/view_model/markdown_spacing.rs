#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum MarkdownElement {
    Paragraph,
    Heading,
    List,
    CodeBlock,
    Table,
    Blockquote,
}

#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum MarkdownSpacingMode {
    #[default]
    Normal,
    Compact,
}

#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct ElementSpacing {
    pub before: Option<u8>,
    pub after: Option<u8>,
}

#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct MarkdownSpacingOverrides {
    pub paragraph: Option<ElementSpacing>,
    pub heading: Option<ElementSpacing>,
    pub list: Option<ElementSpacing>,
    pub code_block: Option<ElementSpacing>,
    pub table: Option<ElementSpacing>,
    pub blockquote: Option<ElementSpacing>,
}

#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct MarkdownSpacingPolicy {
    mode: MarkdownSpacingMode,
    overrides: MarkdownSpacingOverrides,
}

impl MarkdownSpacingPolicy {
    /// `Default` 的常量形态（测试 const fn 求值需要）。
    #[cfg(test)]
    pub const fn normal() -> Self {
        Self {
            mode: MarkdownSpacingMode::Normal,
            overrides: MarkdownSpacingOverrides {
                paragraph: None,
                heading: None,
                list: None,
                code_block: None,
                table: None,
                blockquote: None,
            },
        }
    }

    #[cfg(test)]
    pub const fn compact() -> Self {
        Self {
            mode: MarkdownSpacingMode::Compact,
            ..Self::normal()
        }
    }

    #[cfg(test)]
    pub const fn new_for_test(
        mode: MarkdownSpacingMode,
        overrides: MarkdownSpacingOverrides,
    ) -> Self {
        Self { mode, overrides }
    }

    pub const fn mode(self) -> MarkdownSpacingMode {
        self.mode
    }

    pub const fn overrides(self) -> MarkdownSpacingOverrides {
        self.overrides
    }

    pub fn boundary_gap(
        self,
        left: MarkdownElement,
        right: MarkdownElement,
        source_gap: bool,
    ) -> u8 {
        let left_after = self.element(left).and_then(|spacing| spacing.after);
        let right_before = self.element(right).and_then(|spacing| spacing.before);
        if left_after.is_some() || right_before.is_some() {
            left_after.unwrap_or(0).max(right_before.unwrap_or(0))
        } else if self.mode == MarkdownSpacingMode::Normal && source_gap {
            1
        } else {
            0
        }
    }

    pub fn leading_gap(self, element: MarkdownElement) -> u8 {
        self.element(element)
            .and_then(|spacing| spacing.before)
            .unwrap_or(0)
    }

    pub fn trailing_gap(self, element: MarkdownElement) -> u8 {
        self.element(element)
            .and_then(|spacing| spacing.after)
            .unwrap_or(0)
    }

    fn element(self, element: MarkdownElement) -> Option<ElementSpacing> {
        match element {
            MarkdownElement::Paragraph => self.overrides.paragraph,
            MarkdownElement::Heading => self.overrides.heading,
            MarkdownElement::List => self.overrides.list,
            MarkdownElement::CodeBlock => self.overrides.code_block,
            MarkdownElement::Table => self.overrides.table,
            MarkdownElement::Blockquote => self.overrides.blockquote,
        }
    }
}

impl From<&sdk::ConfigView> for MarkdownSpacingPolicy {
    fn from(view: &sdk::ConfigView) -> Self {
        Self {
            mode: match view.markdown_spacing {
                sdk::MarkdownSpacingModeView::Normal => MarkdownSpacingMode::Normal,
                sdk::MarkdownSpacingModeView::Compact => MarkdownSpacingMode::Compact,
            },
            overrides: overrides_from_sdk(view.markdown_spacing_overrides),
        }
    }
}

fn overrides_from_sdk(value: sdk::MarkdownSpacingOverridesView) -> MarkdownSpacingOverrides {
    MarkdownSpacingOverrides {
        paragraph: value.paragraph.map(element_from_sdk),
        heading: value.heading.map(element_from_sdk),
        list: value.list.map(element_from_sdk),
        code_block: value.code_block.map(element_from_sdk),
        table: value.table.map(element_from_sdk),
        blockquote: value.blockquote.map(element_from_sdk),
    }
}

fn element_from_sdk(value: sdk::ElementSpacingView) -> ElementSpacing {
    ElementSpacing {
        before: value.before.filter(|value| *value <= 8),
        after: value.after.filter(|value| *value <= 8),
    }
}

#[cfg(test)]
#[path = "markdown_spacing_tests.rs"]
mod tests;
