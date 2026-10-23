// Wlel grammar for tree-sitter — https://github.com/WildanDeveloper/wlel
//
// Mirrors the reference grammar in docs/spec.md §16. Wlel is deliberately
// small: no macros, no operator overloading, and `defer`/`arena` are
// statement forms.
//
// One wrinkle needs care. `Name { field: value }` is a struct literal, so any
// expression that ends in a name could swallow the `{` that opens a following
// block: `while i < n { ... }` would read `n { ... }` as a struct. The
// reference parser resolves this with a three-token lookahead (`{ IDENT :`).
// Here the same job is done by keeping struct literals out of the expression
// rule used for conditions and operands (`_control_expression`); a struct
// literal that really is a condition can be parenthesized, exactly as the
// reference parser requires.

module.exports = grammar({
  name: 'wlel',

  extras: $ => [
    /\s/,
    $.line_comment,
    $.doc_comment,
  ],

  word: $ => $.identifier,

  conflicts: $ => [
    // `a[i]` and `vec_new[int](...)` are the same `name [` prefix; only the
    // token after `]` tells an index from a type-argument list
    [$._control_expression, $.named_type],
  ],

  rules: {
    source_file: $ => repeat($._item),

    _item: $ => choice(
      $.use_declaration,
      $.function_declaration,
      $.extern_declaration,
      $.struct_declaration,
      $.enum_declaration,
      $.impl_declaration,
      $.test_declaration,
    ),

    // ---- comments ---------------------------------------------------------

    // `///` is a doc comment. Both forms match the same text, so the plain
    // comment is given a lower lexical precedence instead of relying on rule
    // order.
    line_comment: _ => token(prec(-1, seq('//', /[^\n]*/))),
    doc_comment: _ => token(seq('///', /[^\n]*/)),

    // ---- declarations -----------------------------------------------------

    use_declaration: $ => seq(
      'use',
      choice($.string_literal, $.std_module),
      ';',
    ),

    std_module: _ => 'std',

    function_declaration: $ => seq(
      'fn',
      field('name', $.identifier),
      optional(field('type_params', $.type_parameters)),
      field('parameters', $.parameter_list),
      optional(seq('->', field('return_type', $._type))),
      field('body', $.block),
    ),

    extern_declaration: $ => seq(
      'extern',
      'fn',
      field('name', $.identifier),
      field('parameters', $.parameter_list),
      optional(seq('->', field('return_type', $._type))),
      ';',
    ),

    struct_declaration: $ => seq(
      'struct',
      field('name', $.identifier),
      optional(field('type_params', $.type_parameters)),
      '{',
      optional(seq($.field_declaration, repeat(seq(',', $.field_declaration)), optional(','))),
      '}',
    ),

    field_declaration: $ => seq(
      field('name', $.identifier),
      ':',
      field('type', $._type),
    ),

    enum_declaration: $ => seq(
      'enum',
      field('name', $.identifier),
      optional(field('type_params', $.type_parameters)),
      '{',
      optional(seq($.variant, repeat(seq(',', $.variant)), optional(','))),
      '}',
    ),

    variant: $ => seq(
      field('name', $.identifier),
      optional(seq('(', optional(seq($._type, repeat(seq(',', $._type)), optional(','))), ')')),
    ),

    impl_declaration: $ => seq(
      'impl',
      field('type', $.identifier),
      optional(field('type_params', $.type_parameters)),
      '{',
      repeat($.function_declaration),
      '}',
    ),

    test_declaration: $ => seq(
      'test',
      field('name', $.string_literal),
      field('body', $.block),
    ),

    type_parameters: $ => seq(
      '[',
      seq($.identifier, repeat(seq(',', $.identifier)), optional(',')),
      ']',
    ),

    parameter_list: $ => seq(
      '(',
      optional(seq($.parameter, repeat(seq(',', $.parameter)), optional(','))),
      ')',
    ),

    parameter: $ => seq(
      field('name', $.identifier),
      optional(seq(':', field('type', $._type))),
    ),

    // ---- types ------------------------------------------------------------

    _type: $ => choice(
      $.primitive_type,
      $.pointer_type,
      $.array_type,
      $.generic_type,
      $.named_type,
    ),

    primitive_type: _ => choice(
      'int', 'i8', 'i16', 'i32', 'i64',
      'u8', 'u16', 'u32', 'u64', 'usize',
      'f32', 'f64', 'float', 'bool', 'string', 'byte', 'char', 'void',
    ),

    pointer_type: $ => prec(2, seq('*', $._type)),

    array_type: $ => seq('[', field('element', $._type), ';', field('length', $.integer_literal), ']'),

    generic_type: $ => prec(1, seq(
      field('name', $.identifier),
      '[',
      seq($._type, repeat(seq(',', $._type)), optional(',')),
      ']',
    )),

    named_type: $ => $.identifier,

    // ---- statements -------------------------------------------------------

    block: $ => seq('{', repeat($._statement), '}'),

    block_expression: $ => prec(1, seq('{', repeat($._statement), '}')),

    _statement: $ => choice(
      $.let_declaration,
      $.short_declaration,
      $.assignment,
      $.compound_assignment,
      $.return_statement,
      $.if_statement,
      $.while_statement,
      $.for_statement,
      $.break_statement,
      $.continue_statement,
      $.defer_statement,
      $.arena_statement,
      $.expression_statement,
      // statement form: `match x { ... }` with no trailing `;` (spec §8.2)
      $.match_expression,
      $.block,
    ),

    let_declaration: $ => seq(
      'let',
      field('name', $.identifier),
      optional(seq(':', field('type', $._type))),
      choice(':=', '='),
      field('value', $._expression),
      ';',
    ),

    // `x := expr;` declares without the `let` prefix (spec §6.1); `x = expr;`
    // is an assignment, so only `:=` belongs here
    short_declaration: $ => seq(
      field('name', $.identifier),
      ':=',
      field('value', $._expression),
      ';',
    ),

    assignment: $ => seq(
      field('target', $._expression),
      '=',
      field('value', $._expression),
      ';',
    ),

    compound_assignment: $ => seq(
      field('target', $._expression),
      field('operator', choice('+=', '-=', '*=', '/=', '%=')),
      field('value', $._expression),
      ';',
    ),

    return_statement: $ => seq('return', optional($._expression), ';'),

    if_statement: $ => prec.right(20, seq(
      'if',
      field('condition', $._control_expression),
      field('consequence', $.block),
      optional(seq('else', choice($.block, $.if_statement))),
    )),

    while_statement: $ => seq(
      'while',
      field('condition', $._control_expression),
      field('body', $.block),
    ),

    for_statement: $ => seq(
      'for',
      field('variable', $.identifier),
      'in',
      choice(field('range', $.range_expression), field('iterable', $._control_expression)),
      field('body', $.block),
    ),

    range_expression: $ => prec.left(seq(
      field('start', $._control_expression),
      '..',
      field('end', $._control_expression),
    )),

    break_statement: _ => seq('break', ';'),
    continue_statement: _ => seq('continue', ';'),

    defer_statement: $ => seq(
      'defer',
      choice($.block, seq($._expression, ';')),
    ),

    arena_statement: $ => seq(
      'arena',
      optional(seq('(', field('size', $._expression), ')')),
      field('body', $.block),
    ),

    expression_statement: $ => seq($._expression, ';'),

    // ---- expressions ------------------------------------------------------

    // Full expression: everywhere a `{` after a name can only be a struct
    // literal (right of `=`, `:=`, `return`, an argument, a field value).
    _expression: $ => choice(
      $._control_expression,
      $.struct_literal,
      $.match_expression,
      $.block_expression,
    ),

    // No struct literal. Used for conditions and for the operands of binary,
    // unary and cast expressions, so the brace after `while i < n` opens the
    // loop body.
    _control_expression: $ => choice(
      $.identifier,
      $.path_expression,
      $.integer_literal,
      $.float_literal,
      $.string_literal,
      $.boolean_literal,
      $.self_expression,
      $.binary_expression,
      $.unary_expression,
      $.call_expression,
      $.method_call,
      $.field_expression,
      $.index_expression,
      $.cast_expression,
      $.array_literal,
      $.try_expression,
      $.new_expression,
      $.sizeof_expression,
      $.arena_handle,
      $.parenthesized_expression,
    ),

    self_expression: _ => 'self',

    parenthesized_expression: $ => seq('(', $._expression, ')'),

    unary_expression: $ => prec.right(16, seq(
      field('operator', choice('!', '-', '~', '&', '*')),
      field('operand', $._control_expression),
    )),

    binary_expression: $ => {
      // tightest to loosest, every level left-associative (spec §5.1)
      const table = [
        [13, '*', '/', '%'],
        [12, '+', '-'],
        [11, '<<', '>>'],
        [9, '==', '!=', '<', '>', '<=', '>='],
        [8, '&'],
        [7, '^'],
        [6, '|'],
        [5, '&&'],
        [4, '||'],
      ];
      return choice(...table.map(([precedence, ...ops]) =>
        prec.left(precedence, seq(
          field('left', $._control_expression),
          field('operator', ops.length === 1 ? ops[0] : choice(...ops)),
          field('right', $._control_expression),
        ))
      ));
    },

    // `std::println_int`, `std::fs::open`, `sys::chan_new` — one or more
    // module segments before the name
    path_expression: $ => prec(1, seq(
      field('module', $.identifier),
      repeat1(seq('::', field('name', $.identifier))),
    )),

    // Any postfix expression can be called: `f(x)`, `std::println_int(x)`,
    // and `vec_new[int](4)` — whose type arguments are an index_expression
    // whose content happens to be a type.
    call_expression: $ => prec.left(17, seq(
      field('function', $._control_expression),
      field('arguments', $.argument_list),
    )),

    argument_list: $ => seq(
      '(',
      optional(seq($._expression, repeat(seq(',', $._expression)), optional(','))),
      ')',
    ),

    method_call: $ => prec.left(19, seq(
      field('receiver', $._control_expression),
      '.',
      field('method', $.identifier),
      field('arguments', $.argument_list),
    )),

    field_expression: $ => prec.left(18, seq(
      field('receiver', $._control_expression),
      '.',
      field('field', $.identifier),
    )),

    index_expression: $ => prec.left(16, seq(
      field('value', $._control_expression),
      choice(
        seq('[', field('index', $._expression), ']'),
        field('type_arguments', $.type_arguments),
      ),
    )),

    // `vec_new[int](4)`, `map_new[string, Jval]()`: the brackets of a call
    type_arguments: $ => seq(
      '[',
      seq($._type, repeat(seq(',', $._type)), optional(',')),
      ']',
    ),

    cast_expression: $ => prec(14, seq(
      field('value', $._control_expression),
      'as',
      field('type', $._type),
    )),

    try_expression: $ => prec(15, seq($._control_expression, '?')),

    struct_literal: $ => prec(1, seq(
      field('name', $.identifier),
      '{',
      optional(seq($.field_initializer, repeat(seq(',', $.field_initializer)), optional(','))),
      '}',
    )),

    field_initializer: $ => seq(
      field('name', $.identifier),
      ':',
      field('value', $._expression),
    ),

    array_literal: $ => seq(
      '[',
      optional(seq($._expression, repeat(seq(',', $._expression)), optional(','))),
      ']',
    ),

    match_expression: $ => prec(3, seq(
      'match',
      field('value', $._control_expression),
      '{',
      repeat($.match_arm),
      '}',
    )),

    match_arm: $ => seq(
      field('pattern', $.pattern),
      '=>',
      field('value', choice($._expression, $.block)),
      optional(','),
    ),

    // `_`, a bare name (zero-payload variant or a binding), or
    // `Variant(a, b, ...)`
    pattern: $ => choice(
      $.wildcard_pattern,
      $.variant_pattern,
      $.identifier,
    ),

    wildcard_pattern: _ => '_',

    variant_pattern: $ => seq(
      field('variant', $.identifier),
      '(',
      optional(seq($.identifier, repeat(seq(',', $.identifier)), optional(','))),
      ')',
    ),

    new_expression: $ => prec(15, seq(
      'new',
      '(',
      field('type', $._type),
      optional(seq(',', field('count', $._expression))),
      ')',
    )),

    sizeof_expression: $ => seq(
      'wlel_sizeof',
      '(',
      field('type', $._type),
      ')',
    ),

    arena_handle: _ => seq('wlel_arena', '(', ')'),

    // ---- literals ---------------------------------------------------------

    boolean_literal: _ => choice('true', 'false'),

    integer_literal: _ => token(choice(
      // hex / binary / octal, with optional width suffix
      /0[xX][0-9a-fA-F][0-9a-fA-F_]*(i8|i16|i32|i64|u8|u16|u32|u64|usize)?/,
      /0[bB][01][01_]*(i8|i16|i32|i64|u8|u16|u32|u64|usize)?/,
      /0[oO][0-7][0-7_]*(i8|i16|i32|i64|u8|u16|u32|u64|usize)?/,
      // decimal, with optional width suffix
      /[0-9][0-9_]*(i8|i16|i32|i64|u8|u16|u32|u64|usize|byte|char)?/,
    )),

    float_literal: _ => token(choice(
      /[0-9][0-9_]*\.[0-9][0-9_]*([eE][+-]?[0-9]+)?(f32|f64)?/,
      /[0-9][0-9_]*[eE][+-]?[0-9]+(f32|f64)?/,
    )),

    string_literal: _ => token(seq(
      '"',
      repeat(choice(
        /[^"\\\n]+/,
        /\\(n|t|r|"|\\|0)/,
      )),
      '"',
    )),

    // Reserved words are plain string tokens; giving the identifier a lower
    // lexical precedence means they can never be read as names, which is what
    // makes `match x { ... }` a match and not a struct literal called `match`.
    identifier: _ => token(prec(-1, /[A-Za-z_][A-Za-z0-9_]*/)),
  },
});
