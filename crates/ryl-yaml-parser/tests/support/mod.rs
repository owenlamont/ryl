#![allow(dead_code)]

pub fn is_comment_event(event: &ryl_yaml_parser::Event<'_>) -> bool {
    matches!(event, ryl_yaml_parser::Event::Comment(..))
}

pub fn is_comment_token(token: &ryl_yaml_parser::TokenType<'_>) -> bool {
    matches!(token, ryl_yaml_parser::TokenType::Comment(_))
}

pub fn comment_modes() -> &'static [bool] {
    &[true, false]
}
