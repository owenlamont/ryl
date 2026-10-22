#[path = "scanner/block_scalar_chunks.rs"]
mod block_scalar_chunks;
#[path = "scanner/block_scalar_indentation.rs"]
mod block_scalar_indentation;
#[path = "scanner/block_scalar_keep.rs"]
mod block_scalar_keep;
#[path = "scanner/blocks.rs"]
mod blocks;
#[path = "scanner/closing_bracket.rs"]
mod closing_bracket;
#[path = "scanner/empty_lines_and_chomping.rs"]
mod empty_lines_and_chomping;
#[path = "scanner/indentation.rs"]
mod indentation;
#[path = "scanner/misplaced_flow_chars.rs"]
mod misplaced_flow_chars;
#[path = "scanner/non_break_scan.rs"]
mod non_break_scan;
#[path = "scanner/plain_scalar_indicators.rs"]
mod plain_scalar_indicators;
#[path = "scanner/plain_tab.rs"]
mod plain_tab;
#[path = "scanner/printable.rs"]
mod printable;
#[path = "scanner/quoted_scalar_chunks.rs"]
mod quoted_scalar_chunks;
#[path = "scanner/simple_key_bookkeeping.rs"]
mod simple_key_bookkeeping;
mod support;
#[path = "scanner/surrogates.rs"]
mod surrogates;
#[path = "scanner/tags_and_flow.rs"]
mod tags_and_flow;
#[path = "scanner/token_observer.rs"]
mod token_observer;
#[path = "scanner/tokens.rs"]
mod tokens;
#[path = "scanner/unclosed_flow.rs"]
mod unclosed_flow;
#[path = "scanner/whitespace_result.rs"]
mod whitespace_result;
