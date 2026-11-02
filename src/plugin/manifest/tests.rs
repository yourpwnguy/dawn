use super::{Anchor, is_granted, normalize_relative, parse};
use crate::config::{ClockStyle, WidgetLayer};

const BASE: &str = r##"
api_version = 1
name = "Hello"
version = "1.0.0"

[permissions]
filesystem = ["assets"]

[[widget]]
kind = "text"
text = "Welcome"
"##;

fn color() -> crate::color::Rgb {
  crate::color::Rgb::solid(1, 2, 3)
}

#[test]
fn parses_a_minimal_manifest() {
  let manifest = parse(BASE, color()).unwrap();
  assert_eq!(manifest.name, "Hello");
  assert_eq!(manifest.version, "1.0.0");
  assert_eq!(manifest.widgets.len(), 1);
  assert_eq!(manifest.grants, ["assets"]);
}

#[test]
fn rejects_unsupported_api_versions() {
  let error = parse("api_version = 2\nname = \"X\"\n", color()).unwrap_err();
  assert!(error.contains("not supported"));
}

#[test]
fn rejects_unknown_fields_loudly() {
  let error = parse("api_version = 1\nname = \"X\"\nnetwrok = true\n", color()).unwrap_err();
  assert!(error.contains("netwrok"));
}

#[test]
fn rejects_no_code_permissions() {
  let error = parse(
    "api_version = 1\nname = \"X\"\n[permissions]\nnetwork = true\n",
    color(),
  )
  .unwrap_err();
  assert!(error.contains("network"));
}

#[test]
fn skips_a_bad_widget_but_keeps_the_plugin() {
  let source = format!("{BASE}\n[[widget]]\nkind = \"text\"\ntext = \"Too big\"\nsize = 1000\n");
  let manifest = parse(&source, color()).unwrap();
  assert_eq!(manifest.widgets.len(), 1);
}

#[test]
fn widget_layer_parses_and_an_unknown_value_skips_the_widget() {
  let source =
    format!("{BASE}\n[[widget]]\nkind = \"text\"\ntext = \"Front\"\nlayer = \"front\"\n");
  let manifest = parse(&source, color()).unwrap();
  assert_eq!(manifest.widgets.len(), 2);
  match &manifest.widgets[1] {
    super::ParsedWidget::Text { layer, .. } => assert_eq!(*layer, WidgetLayer::Front),
    other => panic!("expected a text widget, got {other:?}"),
  }

  // The default stays behind, and an unknown layer only removes the widget
  // that wrote it. The rest of the manifest is still usable.
  let defaulted = parse(BASE, color()).unwrap();
  match &defaulted.widgets[0] {
    super::ParsedWidget::Text { layer, .. } => assert_eq!(*layer, WidgetLayer::Behind),
    other => panic!("expected a text widget, got {other:?}"),
  }
  let bad = format!("{BASE}\n[[widget]]\nkind = \"text\"\ntext = \"Nope\"\nlayer = \"middle\"\n");
  let manifest = parse(&bad, color()).unwrap();
  assert_eq!(manifest.widgets.len(), 1);
}

#[test]
fn clock_widgets_support_the_extended_v1_fields() {
  let source = r#"
api_version = 1
name = "Clock"
[[widget]]
kind = "clock"
style = "analog"
seconds = true
hour12 = true
date_format = "%Y"
timezone = "UTC"
monitor = "DP-1"
"#;
  let manifest = parse(source, color()).unwrap();
  match &manifest.widgets[0] {
    super::ParsedWidget::Clock { spec, monitor, .. } => {
      assert_eq!(spec.style, ClockStyle::Analog);
      assert!(spec.seconds);
      assert!(spec.timezone.is_some());
      assert_eq!(monitor.as_deref(), Some("DP-1"));
    }
    other => panic!("expected a clock, got {other:?}"),
  }
}

#[test]
fn media_widgets_use_the_same_manifest_surface() {
  let source = r#"
api_version = 1
name = "Media"
[[widget]]
kind = "media"
size = 18
show_art = false
show_before_login = false
anchor = "bottom-left"
"#;
  let manifest = parse(source, color()).unwrap();
  match &manifest.widgets[0] {
    super::ParsedWidget::Media {
      size,
      show_art,
      show_before_login,
      anchor,
      ..
    } => {
      assert_eq!(*size, 18.0);
      assert!(!show_art);
      assert!(!show_before_login);
      assert_eq!(*anchor, Anchor::BottomLeft);
    }
    other => panic!("expected media, got {other:?}"),
  }

  // The pre-auth default matches the built-in widget. A shared machine has
  // to opt out explicitly, in either the main config or the manifest.
  let defaulted = parse(
    r#"
api_version = 1
name = "Media"
[[widget]]
kind = "media"
"#,
    color(),
  )
  .unwrap();
  match &defaulted.widgets[0] {
    super::ParsedWidget::Media {
      show_before_login, ..
    } => assert!(show_before_login),
    other => panic!("expected media, got {other:?}"),
  }
}

#[test]
fn normalises_safe_relative_paths_and_rejects_escapes() {
  assert!(
    normalize_relative("assets/../x", "path")
      .unwrap_err()
      .contains("relative")
  );
  assert!(
    normalize_relative("/etc/passwd", "path")
      .unwrap_err()
      .contains("relative")
  );
  assert_eq!(
    normalize_relative("./assets/logo.png", "path").unwrap(),
    "assets/logo.png"
  );
  assert!(
    normalize_relative("", "path")
      .unwrap_err()
      .contains("empty")
  );
}

#[test]
fn grants_cover_files_and_directories_but_not_siblings() {
  let grants = vec!["assets".to_string(), "logo.png".to_string()];
  assert!(is_granted("assets/logo.png", &grants));
  assert!(is_granted("logo.png", &grants));
  assert!(!is_granted("assets2/logo.png", &grants));
  assert!(!is_granted("secrets.png", &grants));
}

#[test]
fn anchor_positions_respect_offsets() {
  let (x, y) = Anchor::TopLeft.place((100.0, 50.0), (10.0, 5.0), (2, 3));
  assert_eq!((x, y), (2.0, 3.0));
  let (x, y) = Anchor::BottomRight.place((100.0, 50.0), (10.0, 5.0), (2, 3));
  assert_eq!((x, y), (88.0, 42.0));
}
