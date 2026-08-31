use super::{MediaType, MediaTypeError};
use crate::constants::{MAX_MEDIA_TYPE_NAME_BYTES, MAX_MEDIA_TYPE_PARAMETERS};

#[test]
fn parses_and_normalizes_the_essence() {
    let media_type: MediaType = " Text/HTML ".parse().expect("valid");
    assert_eq!(media_type.essence(), "text/html");
    assert_eq!(media_type.top_level(), "text");
    assert_eq!(media_type.subtype(), "html");
    assert_eq!(media_type.structured_suffix(), None);
    assert_eq!(media_type.parameters().count(), 0);
    assert_eq!(media_type.to_string(), "text/html");
}

#[test]
fn exposes_the_structured_syntax_suffix() {
    let media_type = MediaType::parse("application/ld+json").expect("valid");
    assert_eq!(media_type.structured_suffix(), Some("json"));
    let media_type = MediaType::parse("image/svg+xml").expect("valid");
    assert_eq!(media_type.structured_suffix(), Some("xml"));
}

#[test]
fn normalizes_parameters_by_name_and_case() {
    let left = MediaType::parse("text/plain; Charset=UTF-8; format=flowed").expect("valid");
    let right = MediaType::parse("TEXT/PLAIN;format=flowed;charset=UTF-8").expect("valid");
    assert_eq!(left, right);
    assert_eq!(left.parameter("CHARSET"), Some("UTF-8"));
    assert_eq!(left.parameter("missing"), None);
    assert_eq!(left.to_string(), "text/plain;charset=UTF-8;format=flowed");
}

#[test]
fn unquotes_and_requotes_parameter_values() {
    let media_type =
        MediaType::parse(r#"multipart/form-data; boundary="a b\"c\\d""#).expect("valid");
    assert_eq!(media_type.parameter("boundary"), Some(r#"a b"c\d"#));
    assert_eq!(
        media_type.to_string(),
        r#"multipart/form-data;boundary="a b\"c\\d""#
    );
    let reparsed = MediaType::parse(&media_type.to_string()).expect("round-trips");
    assert_eq!(reparsed, media_type);
}

#[test]
fn rejects_grammar_violations_with_positions() {
    assert_eq!(MediaType::parse(""), Err(MediaTypeError::Empty));
    assert_eq!(MediaType::parse("   "), Err(MediaTypeError::Empty));
    assert_eq!(MediaType::parse("text"), Err(MediaTypeError::MissingSlash));
    assert_eq!(
        MediaType::parse("/plain"),
        Err(MediaTypeError::InvalidTopLevel { offset: 0 })
    );
    assert_eq!(
        MediaType::parse("te xt/plain"),
        Err(MediaTypeError::InvalidTopLevel { offset: 2 })
    );
    assert_eq!(
        MediaType::parse("text/"),
        Err(MediaTypeError::InvalidSubtype { offset: 5 })
    );
    assert_eq!(
        MediaType::parse(" text/pl@in"),
        Err(MediaTypeError::InvalidSubtype { offset: 8 })
    );
    assert_eq!(
        MediaType::parse("text/-plain"),
        Err(MediaTypeError::InvalidSubtype { offset: 5 })
    );
}

#[test]
fn rejects_over_long_names() {
    let long = "a".repeat(MAX_MEDIA_TYPE_NAME_BYTES + 1);
    assert_eq!(
        MediaType::parse(&format!("{long}/plain")),
        Err(MediaTypeError::NameTooLong {
            component: "type",
            limit: MAX_MEDIA_TYPE_NAME_BYTES
        })
    );
    assert_eq!(
        MediaType::parse(&format!("text/{long}")),
        Err(MediaTypeError::NameTooLong {
            component: "subtype",
            limit: MAX_MEDIA_TYPE_NAME_BYTES
        })
    );
    let longest = "a".repeat(MAX_MEDIA_TYPE_NAME_BYTES);
    assert!(MediaType::parse(&format!("{longest}/{longest}")).is_ok());
}

#[test]
fn rejects_malformed_parameters_with_positions() {
    assert_eq!(
        MediaType::parse("text/plain;"),
        Err(MediaTypeError::MalformedParameter { offset: 11 })
    );
    assert_eq!(
        MediaType::parse("text/plain; charset"),
        Err(MediaTypeError::MalformedParameter { offset: 19 })
    );
    assert_eq!(
        MediaType::parse("text/plain; charset="),
        Err(MediaTypeError::MalformedParameter { offset: 20 })
    );
    assert_eq!(
        MediaType::parse("text/plain; =utf-8"),
        Err(MediaTypeError::MalformedParameter { offset: 12 })
    );
    assert_eq!(
        MediaType::parse("text/plain; charset=\"open"),
        Err(MediaTypeError::MalformedParameter { offset: 25 })
    );
    assert_eq!(
        MediaType::parse("text/plain; charset=utf-8 junk"),
        Err(MediaTypeError::MalformedParameter { offset: 26 })
    );
}

#[test]
fn rejects_duplicate_and_excess_parameters() {
    assert_eq!(
        MediaType::parse("text/plain; charset=utf-8; CHARSET=ascii"),
        Err(MediaTypeError::DuplicateParameter {
            name: "charset".to_owned()
        })
    );
    let parameters: Vec<String> = (0..=MAX_MEDIA_TYPE_PARAMETERS)
        .map(|index| format!("p{index}=v"))
        .collect();
    assert_eq!(
        MediaType::parse(&format!("text/plain;{}", parameters.join(";"))),
        Err(MediaTypeError::TooManyParameters {
            limit: MAX_MEDIA_TYPE_PARAMETERS
        })
    );
    assert!(
        MediaType::parse(&format!(
            "text/plain;{}",
            parameters[..MAX_MEDIA_TYPE_PARAMETERS].join(";")
        ))
        .is_ok()
    );
}
