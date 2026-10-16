// SPDX-License-Identifier: Apache-2.0
//! Entry boundaries over internal parsers for the `cadmpeg-fuzz` targets.
//!
//! Each wrapper feeds arbitrary bytes to one internal parser. The contract is
//! that no input may panic.
#![doc(hidden)]

/// Exercise STEP lexical scanning.
pub fn lex(data: &[u8]) -> Result<(), cadmpeg_core::CodecError> {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::default();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(data, &arena, &policy)?;
    let mut lexer = crate::lex::Lexer::new(data, &ctx);
    while lexer
        .next_token()
        .map_err(crate::lex::LexError::into_codec_error)?
        .is_some()
    {}
    Ok(())
}

/// Exercise STEP entity parsing.
pub fn parse(data: &[u8]) {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::default();
    let Ok((ctx, _)) = cadmpeg_core::decode::DecodeContext::from_root_bytes(data, &arena, &policy)
    else {
        return;
    };
    let _probe = crate::parse::parse_with_context(data, &ctx);
}

/// Entity count for the parse benchmark; hides the typed exchange.
#[doc(hidden)]
pub fn parse_entity_count(data: &[u8]) -> Result<usize, cadmpeg_core::CodecError> {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::default();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(data, &arena, &policy)?;
    crate::parse::parse_with_context(data, &ctx).map(|(exchange, _)| exchange.records().len())
}

#[cfg(test)]
mod tests {
    use cadmpeg_ir::CadIr;

    #[test]
    fn wrappers_accept_empty() {
        assert!(super::lex(&[]).is_ok());
        super::parse(&[]);
    }

    #[test]
    fn wrappers_accept_exported_document() {
        let source = crate::test_support::exchange::export(&CadIr::empty());
        assert!(super::lex(source.as_bytes()).is_ok());
        super::parse(source.as_bytes());
        assert!(super::parse_entity_count(source.as_bytes()).is_ok());
    }

    #[test]
    fn parse_entry_uses_default_parameter_depth_limit() {
        fn exchange(depth: usize) -> String {
            format!(
                "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM({}1{});ENDSEC;END-ISO-10303-21;",
                "(".repeat(depth),
                ")".repeat(depth),
            )
        }

        assert_eq!(
            super::parse_entity_count(exchange(1).as_bytes()).expect("normal input"),
            1
        );
        let limit = cadmpeg_core::decode::DecodePolicy::default()
            .limits
            .max_recursion_depth;
        let depth = usize::try_from(limit).expect("default depth fits usize");
        assert!(matches!(
            super::parse_entity_count(exchange(depth).as_bytes()),
            Err(cadmpeg_core::CodecError::ResourceLimit(refusal))
                if refusal.dimension == cadmpeg_core::decode::ResourceDimension::RecursionDepth
                    && refusal.limit == limit
        ));
    }

    #[test]
    fn parse_entity_count_returns_parser_error() {
        let error = super::parse_entity_count(b"not a STEP exchange")
            .expect_err("invalid exchange must fail at the parser boundary");
        assert!(matches!(error, cadmpeg_core::CodecError::Malformed(_)));
    }
}
