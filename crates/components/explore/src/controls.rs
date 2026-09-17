use super::Control;

/// Control labels share row measurement in pinned and scrolling layouts.
#[derive(Clone)]
pub(super) struct Button {
    pub(super) text: String,
    pub(super) column: u16,
    pub(super) control: Option<Control>,
}

impl Button {
    pub(super) fn wrap(
        width: u16,
        labels: impl IntoIterator<Item = (String, Option<Control>)>,
    ) -> Vec<Vec<Self>> {
        let mut rows = Vec::new();
        let mut row = Vec::new();
        let mut column: u16 = 0;
        for (text, control) in labels {
            let length = u16::try_from(text.len()).unwrap_or(u16::MAX);
            if column > 0 && column.saturating_add(length) > width {
                rows.push(std::mem::take(&mut row));
                column = 0;
            }
            row.push(Self {
                text,
                column,
                control,
            });
            column = column.saturating_add(length).saturating_add(1);
        }
        if !row.is_empty() {
            rows.push(row);
        }
        rows
    }
}
