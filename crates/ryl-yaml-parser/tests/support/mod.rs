#![allow(dead_code)]

pub fn is_comment_event(event: &granit_parser::Event<'_>) -> bool {
    #[cfg(feature = "comments")]
    {
        matches!(event, granit_parser::Event::Comment(..))
    }
    #[cfg(not(feature = "comments"))]
    {
        let _ = event;
        false
    }
}

pub fn is_comment_token(token: &granit_parser::TokenType<'_>) -> bool {
    #[cfg(feature = "comments")]
    {
        matches!(token, granit_parser::TokenType::Comment(_))
    }
    #[cfg(not(feature = "comments"))]
    {
        let _ = token;
        false
    }
}

pub fn comment_modes() -> &'static [bool] {
    #[cfg(feature = "comments")]
    {
        &[true, false]
    }
    #[cfg(not(feature = "comments"))]
    {
        &[false]
    }
}
