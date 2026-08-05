//! SortBy 解析测试.

use chapbook::sort::{SortBy, SortColumn, SortOrder};

#[test]
fn parse_valid_string() {
    assert_eq!(
        "Name:Asc".parse::<SortBy>(),
        Ok(SortBy {
            column: SortColumn::Name,
            order: SortOrder::Asc
        })
    );
    assert_eq!(
        "Name:Desc".parse::<SortBy>(),
        Ok(SortBy {
            column: SortColumn::Name,
            order: SortOrder::Desc
        })
    );
    assert_eq!(
        "LastModified:Desc".parse::<SortBy>(),
        Ok(SortBy {
            column: SortColumn::LastModified,
            order: SortOrder::Desc
        })
    );
}

#[test]
fn parse_invalid_string() {
    assert!("Name:Invalid".parse::<SortBy>().is_err());
    assert!("Name:Invalid:Ext".parse::<SortBy>().is_err());
}
