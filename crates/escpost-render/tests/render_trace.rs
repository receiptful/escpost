mod support;

use escpost_render::{
    DecodedCommand, Effect, Justification, PaintLifecycle, Position, StateChange,
    TRACED_COMMAND_BYTES, TextFont, render_with_trace,
};
use support::test_profile;

#[test]
fn paint_lifecycle_keeps_unfed_text_buffered_on_a_conceptual_sheet() {
    let profile = test_profile();

    let traced = render_with_trace(b"A", &profile).expect("traced rendering should succeed");

    assert!(traced.render.sheets.is_empty());
    let [sheet] = traced.trace.sheets.as_slice() else {
        panic!("the buffered command should retain its conceptual sheet");
    };
    let [command] = sheet.commands.as_slice() else {
        panic!("the text byte should retain its command trace");
    };
    assert_eq!(command.paint_lifecycle, Some(PaintLifecycle::Buffered));
    assert!(command.effects.is_empty());
}

#[test]
fn paint_lifecycle_promotes_text_when_lf_prints_the_line() {
    let profile = test_profile();

    let traced = render_with_trace(b"A\n", &profile).expect("traced rendering should succeed");

    assert_eq!(
        traced.trace.sheets[0].commands[0].paint_lifecycle,
        Some(PaintLifecycle::Committed)
    );
    assert_eq!(traced.trace.sheets[0].commands[1].paint_lifecycle, None);
}

#[test]
fn paint_lifecycle_marks_raster_images_committed_immediately() {
    let profile = test_profile();
    let input = [0x1d, b'v', b'0', 0, 1, 0, 1, 0, 0x80];

    let traced = render_with_trace(&input, &profile).expect("traced rendering should succeed");

    assert_eq!(
        traced.trace.sheets[0].commands[0].paint_lifecycle,
        Some(PaintLifecycle::Committed)
    );
}

#[test]
fn paint_lifecycle_is_absent_for_state_commands() {
    let profile = test_profile();

    let traced =
        render_with_trace(&[0x1b, b'a', 1], &profile).expect("traced rendering should succeed");

    assert_eq!(traced.trace.sheets[0].commands[0].paint_lifecycle, None);
}

#[test]
fn paint_lifecycle_distinguishes_printed_and_final_buffered_lines() {
    let profile = test_profile();

    let traced = render_with_trace(b"A\nB", &profile).expect("traced rendering should succeed");
    let commands = &traced.trace.sheets[0].commands;

    assert_eq!(commands[0].paint_lifecycle, Some(PaintLifecycle::Committed));
    assert_eq!(commands[1].paint_lifecycle, None);
    assert_eq!(commands[2].paint_lifecycle, Some(PaintLifecycle::Buffered));
    assert!(commands[2].effects.is_empty());
}

#[test]
fn experimental_trace_exposes_sheet_commands_and_logical_bounds() {
    let profile = test_profile();
    let traced = render_with_trace(&[0x1b, b'a', 1, b'A', 0x0a], &profile)
        .expect("traced rendering should succeed");

    assert_eq!(traced.render.sheets.len(), 1);
    assert_eq!(traced.trace.sheets.len(), traced.render.sheets.len());
    let commands = &traced.trace.sheets[0].commands;
    assert_eq!(commands.len(), 3);
    assert_eq!(commands[0].byte_range, 0..3);
    assert_eq!(
        commands[0].command,
        DecodedCommand::SetJustification(Justification::Center)
    );
    assert_eq!(
        commands[0].effects,
        [Effect::StateChange(StateChange::Justification {
            before: Justification::Left,
            after: Justification::Center,
        })]
    );
    assert_eq!(commands[1].byte_range, 3..4);
    assert_eq!(commands[1].command, DecodedCommand::TextByte(b'A'));
    let [Effect::Paint { bounds }] = commands[1].effects.as_slice() else {
        panic!("the printable byte should expose its logical bounds");
    };
    assert_eq!(
        (bounds.x, bounds.y, bounds.width, bounds.height),
        (186, 0, 12, 24)
    );
    assert_eq!(commands[2].byte_range, 4..5);
    assert_eq!(commands[2].command, DecodedCommand::LineFeed);
    assert_eq!(
        commands[2].effects,
        [Effect::Motion {
            before: Position { x: 198, y: 0 },
            after: Position { x: 0, y: 30 },
        }]
    );
}

#[test]
fn ignored_justification_has_no_state_change_effect() {
    let profile = test_profile();
    let traced = render_with_trace(&[b'A', 0x1b, b'a', 1, 0x0a], &profile)
        .expect("traced rendering should succeed");

    let justification = traced.trace.sheets[0]
        .commands
        .iter()
        .find(|command| matches!(command.command, DecodedCommand::SetJustification(_)))
        .expect("ESC a should still have a command entry");
    assert!(
        justification.effects.is_empty(),
        "an ignored ESC a must not claim a state transition"
    );
}

#[test]
fn a_space_has_logical_bounds_without_ink() {
    let profile = test_profile();
    let traced =
        render_with_trace(&[b' ', 0x0a], &profile).expect("traced rendering should succeed");

    let [Effect::Paint { bounds }] = traced.trace.sheets[0].commands[0].effects.as_slice() else {
        panic!("a space should expose its logical character cell");
    };
    assert_eq!(
        (bounds.x, bounds.y, bounds.width, bounds.height),
        (0, 0, 12, 24)
    );
    assert!(!traced.render.sheets[0].surface.is_printed(0, 0));
}

#[test]
fn raster_image_trace_uses_the_complete_logical_image_area() {
    let profile = test_profile();
    let input = [0x1d, b'v', b'0', 0, 1, 0, 2, 0, 0x80, 0x00];

    let traced = render_with_trace(&input, &profile).expect("the raster image should render");
    let [command] = traced.trace.sheets[0].commands.as_slice() else {
        panic!("the raster image should produce one trace command");
    };

    assert_eq!(command.byte_range, 0..input.len());
    assert_eq!(
        command.command,
        DecodedCommand::RasterImage {
            width_dots: 8,
            height_dots: 2,
            horizontal_scale: 1,
            vertical_scale: 1
        }
    );
    let [Effect::Paint { bounds }] = command.effects.as_slice() else {
        panic!("the raster image should expose its logical bounds");
    };
    assert_eq!(
        (bounds.x, bounds.y, bounds.width, bounds.height),
        (0, 0, 8, 2)
    );
}

#[test]
fn qr_trace_attributes_bounds_to_the_print_command_only() {
    let profile = test_profile();
    let input = [
        0x1d, b'(', b'k', 4, 0, 49, 80, 48, b'A', 0x1d, b'(', b'k', 3, 0, 49, 81, 48,
    ];

    let traced = render_with_trace(&input, &profile).expect("the QR code should render");
    let [store, command] = traced.trace.sheets[0].commands.as_slice() else {
        panic!("QR storage and printing should each produce one trace command");
    };

    assert_eq!(store.byte_range, 0..9);
    assert_eq!(store.command, DecodedCommand::StoreQrData(1));
    assert!(store.effects.is_empty());
    assert_eq!(command.byte_range, 9..17);
    assert_eq!(command.command, DecodedCommand::QrCode(vec![b'A']));
    let [Effect::Paint { bounds }] = command.effects.as_slice() else {
        panic!("the QR print operation should expose its logical bounds");
    };
    assert_eq!(
        (bounds.x, bounds.y, bounds.width, bounds.height),
        (0, 0, 63, 63)
    );
}

#[test]
fn commands_are_grouped_under_the_sheet_active_when_they_execute() {
    let profile = test_profile();
    let traced = render_with_trace(&[b'A', 0x0a, 0x1d, b'V', 0, b'B', 0x0a], &profile)
        .expect("traced rendering should succeed");

    assert_eq!(traced.render.sheets.len(), 2);
    assert_eq!(traced.trace.sheets.len(), 2);
    assert_eq!(traced.trace.sheets[0].commands.len(), 3);
    assert_eq!(
        traced.trace.sheets[0].commands[0].command,
        DecodedCommand::TextByte(b'A')
    );
    assert_eq!(
        traced.trace.sheets[0].commands[2].command,
        DecodedCommand::CutPaper {
            full: true,
            feed: None
        }
    );
    assert_eq!(traced.trace.sheets[1].commands.len(), 2);
    assert_eq!(
        traced.trace.sheets[1].commands[0].command,
        DecodedCommand::TextByte(b'B')
    );
}

#[test]
fn a_profile_suppressed_line_feed_has_no_motion_effect() {
    let profile = test_profile();
    let traced = render_with_trace(&[0x1d, b'v', b'0', 0, 1, 0, 1, 0, 0x80, 0x0a], &profile)
        .expect("traced rendering should succeed");

    let line_feed = traced.trace.sheets[0]
        .commands
        .iter()
        .find(|command| command.command == DecodedCommand::LineFeed)
        .expect("LF should retain its command entry");
    assert!(
        line_feed.effects.is_empty(),
        "a profile-suppressed LF must not claim motion"
    );
}

#[test]
fn a_code_table_command_names_the_encoding_the_profile_maps_it_to() {
    let profile = test_profile();

    let traced =
        render_with_trace(&[0x1b, b't', 2], &profile).expect("traced rendering should succeed");

    assert_eq!(
        traced.trace.sheets[0].commands[0].command,
        DecodedCommand::SelectCodeTable {
            table: 2,
            encoding: Some("CP850".to_owned())
        }
    );
}

#[test]
fn a_command_keeps_its_own_bytes_for_the_command_list() {
    let profile = test_profile();

    let traced =
        render_with_trace(&[0x1b, b'a', 1, b'A'], &profile).expect("tracing should succeed");

    assert_eq!(traced.trace.sheets[0].commands[0].bytes, [0x1b, b'a', 1]);
    assert_eq!(traced.trace.sheets[0].commands[1].bytes, [b'A']);
}

#[test]
fn a_long_payload_keeps_only_the_start_of_its_bytes() {
    let profile = test_profile();
    let width_bytes = 8usize;
    let height_dots = 16usize;
    let mut input = vec![
        0x1d,
        b'v',
        b'0',
        0,
        width_bytes as u8,
        0,
        height_dots as u8,
        0,
    ];
    input.resize(input.len() + width_bytes * height_dots, 0x5a);

    let traced = render_with_trace(&input, &profile).expect("the raster image should render");
    let command = &traced.trace.sheets[0].commands[0];

    assert_eq!(command.byte_range, 0..input.len());
    assert_eq!(command.bytes.len(), TRACED_COMMAND_BYTES);
    assert_eq!(command.bytes[..8], input[..8]);
}

#[test]
fn the_first_command_carries_the_style_the_printer_starts_with() {
    let profile = test_profile();

    let traced = render_with_trace(b"A", &profile).expect("tracing should succeed");
    let style = traced.trace.sheets[0].commands[0]
        .style
        .as_ref()
        .expect("the first command carries the style the job starts with");

    assert_eq!(style.font, TextFont::A);
    assert!(!style.emphasized);
    assert_eq!(style.underline_thickness, 0);
    assert_eq!(style.width_magnification, 1);
    assert_eq!(style.height_magnification, 1);
    assert!(!style.reversed);
    assert_eq!(style.justification, Justification::Left);
}

#[test]
fn a_style_rides_only_on_the_command_that_changed_it() {
    let profile = test_profile();
    let input = [b'A', 0x1b, b'E', 1, b'B', 0x1b, b'E', 1, b'C'];

    let traced = render_with_trace(&input, &profile).expect("tracing should succeed");
    let commands = &traced.trace.sheets[0].commands;

    // The first command holds the style the job starts with.
    assert!(commands[0].style.is_some());
    // ESC E turns emphasis on, thus it carries the style it produced.
    assert_eq!(
        commands[1].style.as_ref().map(|style| style.emphasized),
        Some(true)
    );
    // A text byte changes no style of its own.
    assert!(commands[2].style.is_none());
    // The second ESC E asks for emphasis that is already on.
    assert!(commands[3].style.is_none());
    assert!(commands[4].style.is_none());
}

#[test]
fn a_style_carries_the_whole_state_that_prints_a_character() {
    let profile = test_profile();
    // ESC ! sets font, emphasis, height and width at once; ESC a centres.
    let input = [0x1b, b'!', 0x39, 0x1b, b'a', 1, b'A'];

    let traced = render_with_trace(&input, &profile).expect("tracing should succeed");
    let commands = &traced.trace.sheets[0].commands;
    let style = commands[1]
        .style
        .as_ref()
        .expect("ESC a changes the justification");

    assert_eq!(style.font, TextFont::B);
    assert!(style.emphasized);
    assert_eq!(style.height_magnification, 2);
    assert_eq!(style.width_magnification, 2);
    assert_eq!(style.justification, Justification::Center);
}
