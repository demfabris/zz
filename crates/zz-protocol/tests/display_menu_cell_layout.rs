use zz_protocol::{
    MENU_ROW_MARGIN, MenuRowLayout, layout_menu_row, menu_row_cells, menu_row_width, trim_menu_row,
};

fn layout(name: &str, key: Option<&str>, room: usize) -> MenuRowLayout {
    layout_menu_row(name, key, room)
}

#[test]
fn annotates_a_key_that_fits_a_quarter_of_the_room() {
    let row = layout("SHORT", Some("a"), 36);
    assert_eq!(row.name, "SHORT");
    assert_eq!(row.annotation.as_deref(), Some("a"));
    assert_eq!(menu_row_cells(&row.name, row.annotation.as_deref()), 9);
}

#[test]
fn annotates_a_wider_key_only_while_the_whole_name_still_fits_beside_it() {
    let shown = layout("CCCCCCCCCC", Some("M-Enter"), 36);
    assert_eq!(shown.name, "CCCCCCCCCC");
    assert_eq!(shown.annotation.as_deref(), Some("M-Enter"));
    assert_eq!(menu_row_cells(&shown.name, shown.annotation.as_deref()), 20);

    let hidden = layout(&"B".repeat(30), Some("M-Enter"), 36);
    assert_eq!(hidden.name, "B".repeat(30));
    assert_eq!(hidden.annotation, None);
    assert_eq!(
        menu_row_cells(&hidden.name, hidden.annotation.as_deref()),
        30
    );

    let boundary = layout(&"B".repeat(25), Some("M-Enter"), 36);
    assert_eq!(boundary.annotation.as_deref(), Some("M-Enter"));
    let past_boundary = layout(&"B".repeat(26), Some("M-Enter"), 36);
    assert_eq!(past_boundary.annotation, None);
}

#[test]
fn drops_the_annotation_when_the_key_alone_fills_the_room() {
    let row = layout("name", Some("M-Enter"), 9);
    assert_eq!(row.annotation, None);
    assert_eq!(row.name, "name");
}

#[test]
fn trims_an_overlong_name_from_the_left_and_marks_it() {
    let row = layout("ABCDEFGHIJ", None, 5);
    assert_eq!(row.name, "GHIJ>");
    assert_eq!(row.annotation, None);
    assert_eq!(menu_row_width(&row.name), 5);

    let annotated = layout(&"A".repeat(200), Some("a"), 36);
    assert_eq!(annotated.name, format!("{}>", "A".repeat(31)));
    assert_eq!(annotated.annotation.as_deref(), Some("a"));
    assert_eq!(
        menu_row_cells(&annotated.name, annotated.annotation.as_deref()),
        36
    );
    assert_eq!(
        menu_row_cells(&annotated.name, annotated.annotation.as_deref())
            + usize::from(MENU_ROW_MARGIN),
        40
    );
}

#[test]
fn a_disabled_row_never_annotates_its_key() {
    let row = layout("-Disabled", Some("a"), 36);
    assert_eq!(row.name, "-Disabled");
    assert_eq!(row.annotation, None);
}

#[test]
fn measuring_and_trimming_skip_style_runs_and_count_wide_cells() {
    assert_eq!(menu_row_width("#[fg=red]ABC#[default]"), 3);
    assert_eq!(menu_row_width("##x"), 2);
    assert_eq!(menu_row_width("#x"), 2);
    assert_eq!(menu_row_width("あい"), 4);

    assert_eq!(trim_menu_row("#[fg=red]ABCDEFGHIJ", 4), "#[fg=red]GHIJ");
    assert_eq!(trim_menu_row("ABC", 9), "ABC");
    assert_eq!(trim_menu_row("あいうえ", 4), "うえ");

    let styled = layout("#[fg=red]ABCDEFGHIJ", None, 5);
    assert_eq!(styled.name, "#[fg=red]GHIJ>");
    assert_eq!(menu_row_width(&styled.name), 5);
}

#[test]
fn an_empty_key_never_annotates() {
    let row = layout("name", Some(""), 36);
    assert_eq!(row.annotation, None);
    let row = layout("name", None, 36);
    assert_eq!(row.annotation, None);
}
