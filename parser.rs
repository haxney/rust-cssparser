// http://dev.w3.org/csswg/css3-syntax/#tree-construction
//
// The input to the tree construction stage is a sequence of tokens
// from the tokenization stage.
// The output is a tree of items with a stylesheet at the root
// and all other nodes being at-rules, style rules, or declarations.


extern mod core;
use core::util::{swap, replace};
use super::utils::*;
use tokens = super::tokenizer;
use super::ast::*;


pub struct Parser {
    priv tokenizer: ~tokens::Tokenizer,
    priv current_token: Option<tokens::Token>,
    priv errors: ~[~str],
}


impl Parser {
    fn from_tokenizer(tokenizer: ~tokens::Tokenizer) -> ~Parser {
        ~Parser {
            tokenizer: tokenizer,
            current_token: None,
            errors: ~[],
        }
    }
    fn from_str(input: &str) -> ~Parser {
        Parser::from_tokenizer(tokens::Tokenizer::from_str(input))
    }

    // Consume the whole input and return a list of primitives.
    // Could be used for parsing eg. a stand-alone media query.
    // This is similar to consume_simple_block(), but there is no ending token.
    fn parse_primitives(&mut self) -> ~[Primitive] {
        let mut primitives: ~[Primitive] = ~[];
        for self.each_token |parser, token| {
            primitives.push(consume_primitive(parser, token))
        }
        primitives
    }

    fn parse_declarations(&mut self) -> ~[DeclarationBlockItem] {
        consume_declaration_block(self, /* is_nested= */false)
    }

    fn parse_stylesheet(&mut self) -> ~[Rule] {
        consume_top_level_rules(self)
    }
}


//  ***********  End of public API  ***********


impl Parser {
    priv fn consume_token(&mut self) -> tokens::Token {
        let mut current_token = None;
        swap(&mut current_token, &mut self.current_token);
        match current_token {
            Some(token) => token,
            None => {
                let (token, err) = self.tokenizer.next_token();
                match err {
                    Some(ParseError{message: message}) =>
                        // TODO more contextual error handling?
                        self.errors.push(message),
                    None => ()
                }
                token
            }
        }
    }

    priv fn each_token(
        &mut self,
        it: &fn(parser: &mut Parser, token: tokens::Token) -> bool
    ) -> bool {
        loop {
            match self.consume_token() {
                tokens::EOF => return true,
                token => if !it(self, token) { return false },
            }
        }
    }

    // Fail if the is already a "current token".
    // Call it at most once per iteration of .each_token()
    priv fn reconsume_token(&mut self, token: tokens::Token) {
        assert!(self.current_token.is_none());
        self.current_token = Some(token)
    }
}


fn consume_top_level_rules(parser: &mut Parser) -> ~[Rule] {
    let mut rules: ~[Rule] = ~[];
    for parser.each_token |parser, token| {
        match token {
            tokens::WhiteSpace | tokens::CDO | tokens::CDC => (),
            tokens::AtKeyword(name) => rules.push(
                AtRule(consume_at_rule(parser, true, name))),
            token => {
                parser.reconsume_token(token);
                match consume_style_rule(parser, true) {
                    Some(style_rule) => rules.push(QualifiedRule(style_rule)),
                    None => (),
                }
            }
        }
    }
    rules
}


fn consume_at_rule(parser: &mut Parser, is_nested: bool, name: ~str) -> AtRule {
    let mut prelude: ~[Primitive] = ~[];
    let mut block: Option<~[Primitive]> = None;
    for parser.each_token |parser, token| {
        match token {
            tokens::OpenCurlyBraket => block = Some(
                consume_simple_block(parser, tokens::CloseCurlyBraket)),
            tokens::Semicolon => break,
            tokens::CloseCurlyBraket if is_nested
                => { parser.reconsume_token(token); break }
            _ => prelude.push(consume_primitive(parser, token)),
        }
    }
    AtRule {name: name, prelude: prelude, block: block}
}


fn consume_rule_block(parser: &mut Parser) -> ~[Rule] {
    let mut rules: ~[Rule] = ~[];
    for parser.each_token |parser, token| {
        match token {
            tokens::WhiteSpace => (),
            tokens::CloseCurlyBraket => break,
            tokens::AtKeyword(name) => rules.push(
                AtRule(consume_at_rule(parser, true, name))),
            token => {
                parser.reconsume_token(token);
                match consume_style_rule(parser, true) {
                    Some(style_rule) => rules.push(QualifiedRule(style_rule)),
                    None => (),
                }
            }
        }
    }
    rules
}


fn consume_style_rule(parser: &mut Parser, is_nested: bool) -> Option<QualifiedRule> {
    let mut prelude: ~[Primitive] = ~[];
    for parser.each_token |parser, token| {
        match token {
            tokens::OpenCurlyBraket => {
                return Some(QualifiedRule{
                    prelude: replace(&mut prelude, ~[]),
                    block: consume_simple_block(parser, tokens::CloseCurlyBraket)})
            },
            tokens::CloseCurlyBraket if is_nested
                => { parser.reconsume_token(token); break }
            _ => prelude.push(consume_primitive(parser, token)),
        }
    }
    parser.errors.push(~"Missing {} block for style rule");
    None
}


fn consume_declaration_block(parser: &mut Parser, is_nested: bool)
        -> ~[DeclarationBlockItem] {
    let mut items: ~[DeclarationBlockItem] = ~[];
    for parser.each_token |parser, token| {
        match token {
            tokens::WhiteSpace | tokens::Semicolon => (),
            tokens::CloseCurlyBraket if is_nested => break,
            tokens::AtKeyword(name) => items.push(Decl_AtRule(consume_at_rule(parser, true, name))),
            tokens::Ident(name)
            => match consume_declaration(parser, is_nested, name) {
                Some(declaration) => items.push(Declaration(declaration)),
                None => ()
            },
            token => {
                parser.reconsume_token(token);
                consume_declaration_error(parser, is_nested)
            },
        }
    }
    items
}


fn consume_declaration(parser: &mut Parser, is_nested: bool, name: ~str)
        -> Option<Declaration> {
    let mut name = name;  // XXX see replace() below
    for parser.each_token |parser, token| {
        match token {
            tokens::WhiteSpace => (),
            tokens::Colon => {
                return consume_declaration_value(
                    parser, is_nested, replace(&mut name, ~""))
            }
            _ => { parser.reconsume_token(token); break }
        }
    }
    consume_declaration_error(parser, is_nested);
    None
}


fn consume_declaration_value(parser: &mut Parser, is_nested: bool, name: ~str)
        -> Option<Declaration> {
    let mut name = name;  // XXX see replace() below
    let mut value: ~[Primitive] = ~[];
    for parser.each_token |parser, token| {
        match token {
            tokens::CloseCurlyBraket if is_nested
                => { parser.reconsume_token(token); break }
            tokens::Semicolon => break,
            tokens::Delim('!') => {
                return consume_declaration_important(
                    parser, is_nested, replace(&mut name, ~""),
                    replace(&mut value, ~[]))
            },
            _ => value.push(consume_primitive(parser, token)),
        }
    }
    Some(Declaration{name: name, value: value, important: false})
}


fn consume_declaration_important(parser: &mut Parser, is_nested: bool,
                                 name: ~str, value: ~[Primitive])
        -> Option<Declaration> {
    let mut important = false;
    for parser.each_token |parser, token| {
        match token {
            tokens::WhiteSpace => (),
            tokens::Ident(priority) => {
                important = ascii_lower(priority) == ~"important";
                break
            },
            token => { parser.reconsume_token(token); break }
        }
    }
    if !important {
        consume_declaration_error(parser, is_nested);
        return None
    }
    for parser.each_token |parser, token| {
        match token {
            tokens::WhiteSpace => (),
            tokens::CloseCurlyBraket if is_nested
                => { parser.reconsume_token(token); break }
            tokens::Semicolon => break,
            _ => { consume_declaration_error(parser, is_nested); return None }
        }
    }
    Some(Declaration {name: name, value: value, important: true})
}


fn consume_declaration_error(parser: &mut Parser, is_nested: bool) {
    parser.errors.push(~"Invalid declaration");
    for parser.each_token |parser, token| {
        match token {
            tokens::Semicolon => break,
            tokens::CloseCurlyBraket if is_nested
                => { parser.reconsume_token(token); break }
            _ => consume_primitive(parser, token)  // Ignore the return value
        };
    }
}


fn consume_primitive(parser: &mut Parser, first_token: tokens::Token)
        -> Primitive {
    match first_token {
        // Preserved tokens
        tokens::Ident(string) => Ident(string),
        tokens::AtKeyword(string) => AtKeyword(string),
        tokens::Hash(string) => Hash(string),
        tokens::String(string) => String(string),
        tokens::BadString => BadString,
        tokens::URL(string) => URL(string),
        tokens::BadURL => BadURL,
        tokens::Delim(ch) => Delim(ch),
        tokens::Number(value) => Number(value),
        tokens::Percentage(value) => Percentage(value),
        tokens::Dimension(value, unit) => Dimension(value, unit),
        tokens::UnicodeRange{start: s, end: e} => UnicodeRange {start: s, end: e},
        tokens::EmptyUnicodeRange => EmptyUnicodeRange,
        tokens::WhiteSpace => WhiteSpace,
        tokens::Colon => Colon,
        tokens::Semicolon => Semicolon,
        tokens::CDO => CDO,
        tokens::CDC => CDC,
        tokens::CloseSquareBraket => CloseSquareBraket,
        tokens::CloseParenthesis => CloseParenthesis,
        tokens::CloseCurlyBraket => CloseCurlyBraket,

        tokens::OpenSquareBraket => SquareBraketBlock(
            consume_simple_block(parser, tokens::CloseSquareBraket)),
        tokens::OpenParenthesis => ParenthesisBlock(
            consume_simple_block(parser, tokens::CloseParenthesis)),
        tokens::OpenCurlyBraket => CurlyBraketBlock(
            consume_simple_block(parser, tokens::CloseCurlyBraket)),

        tokens::Function(string) => consume_function(parser, string),

        // Getting here is a  programming error.
        tokens::EOF => fail!(),
    }
}


fn consume_simple_block(parser: &mut Parser, ending_token: tokens::Token)
        -> ~[Primitive] {
    let mut value: ~[Primitive] = ~[];
    for parser.each_token |parser, token| {
        if token == ending_token { break }
        else { value.push(consume_primitive(parser, token)) }
    }
    value
}


fn consume_function(parser: &mut Parser, name: ~str)
        -> Primitive {
    let mut current_argument: ~[Primitive] = ~[];
    let mut arguments: ~[~[Primitive]] = ~[];
    for parser.each_token |parser, token| {
        match token {
            tokens::CloseParenthesis => break,
            tokens::Delim(',') => {
                arguments.push(replace(&mut current_argument, ~[]));
            },
            token => current_argument.push(consume_primitive(parser, token)),
        }
    }
    arguments.push(current_argument);
    Function(name, arguments)
}


#[test]
fn test_primitives() {
    fn assert_primitives(input: &str, expected_primitives: &[Primitive],
                         expected_errors: &[~str]) {
        let mut parser = Parser::from_str(input);
        check_results(
            parser.parse_primitives(), expected_primitives,
            parser.errors, expected_errors);
    }

    assert_primitives("", [], []);
    assert_primitives("42 foo([aa ()b], -){\n  }", [
        Number(NumericValue{representation: ~"42", value: 42f64,
                            int_value: Some(42)}),
        WhiteSpace,
        Function(~"foo", ~[
            ~[SquareBraketBlock(~[
                Ident(~"aa"), WhiteSpace, ParenthesisBlock(~[]), Ident(~"b")])],
            ~[WhiteSpace, Delim('-')]]),
        CurlyBraketBlock(~[
            WhiteSpace])], []);
    assert_primitives("'foo", [String(~"foo")], [~"EOF in quoted string"]);
}


#[test]
fn test_declarations() {
    fn assert_declarations(
            input: &str, expected_declarations: &[DeclarationBlockItem],
            expected_errors: &[~str]) {
        let mut parser = Parser::from_str(input);
        check_results(
            parser.parse_declarations(), expected_declarations,
            parser.errors, expected_errors);
    }
    fn decl(name: ~str, value: ~[Primitive]) -> DeclarationBlockItem {
        Declaration(Declaration{name: name, value: value, important: false})
    }
    fn important(name: ~str, value: ~[Primitive]) -> DeclarationBlockItem {
        Declaration(Declaration{name: name, value: value, important: true})
    }

    assert_declarations("", [], []);
    assert_declarations(" \n  ;; ", [], []);
    assert_declarations("color", [], [~"Invalid declaration"]);
    assert_declarations("color/ red", [], [~"Invalid declaration"]);
    assert_declarations("/**/ color:", [decl(~"color", ~[])], []);
    assert_declarations("a /{(;);}b; c:d",
        [decl(~"c", ~[Ident(~"d")])], [~"Invalid declaration"]);
    assert_declarations("color /**/: black,red; ;",
        [decl(~"color", ~[
            WhiteSpace, Ident(~"black"), Delim(','), Ident(~"red")
        ])], []);

    // !important
    assert_declarations("a:!important /**/; b:c!IMPORTant",
        [important(~"a", ~[]), important(~"b", ~[Ident(~"c")])], []);
    assert_declarations("a:!important b:c",
        [], [~"Invalid declaration"]);
    assert_declarations("a:!important} b:c",
        [], [~"Invalid declaration"]);
    assert_declarations("a:!stuff; a:!!;a:!", [], [
        ~"Invalid declaration", ~"Invalid declaration", ~"Invalid declaration"
    ]);
    // XXX See http://lists.w3.org/Archives/Public/www-style/2013Jan/0491.html
    assert_declarations(
        "a:! important; a:!/**/important;a:!/**/ important",
        [important(~"a", ~[]), important(~"a", ~[]), important(~"a", ~[])],
        []);
}
