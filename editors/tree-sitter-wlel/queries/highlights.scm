; Syntax highlighting for Wlel.
;
; Captures follow the conventional names so any theme (and nvim-treesitter)
; picks them up: @keyword, @function, @type, @variable, @string, @number, …

; ---- comments -------------------------------------------------------------

(line_comment) @comment
(doc_comment) @comment.documentation

; ---- declarations ---------------------------------------------------------

(function_declaration
  name: (identifier) @function)

(extern_declaration
  name: (identifier) @function)

(struct_declaration
  name: (identifier) @type)

(enum_declaration
  name: (identifier) @type)

(impl_declaration
  type: (identifier) @type)

(test_declaration
  name: (string_literal) @string.special)

(field_declaration
  name: (identifier) @property)

(variant
  name: (identifier) @constructor)

(type_parameters
  (identifier) @type.parameter)

(parameter
  name: (identifier) @variable.parameter)

(type_arguments
  (primitive_type) @type.builtin)

(type_arguments
  (named_type
    (identifier) @type))

; ---- types ----------------------------------------------------------------

(primitive_type) @type.builtin

(named_type
  (identifier) @type)

(pointer_type) @type

(array_type) @type

(generic_type
  name: (identifier) @type)

; ---- calls and members ----------------------------------------------------

(call_expression
  function: (identifier) @function.call)

(call_expression
  function: (path_expression) @function.call)

(path_expression
  module: (identifier) @namespace)

(method_call
  method: (identifier) @function.method)

(field_expression
  field: (identifier) @property)

(struct_literal
  name: (identifier) @constructor)

(field_initializer
  name: (identifier) @property)

; ---- patterns -------------------------------------------------------------

(variant_pattern
  variant: (identifier) @constructor)

(pattern
  (identifier) @variable)

(wildcard_pattern) @variable.builtin

; ---- operators and punctuation -------------------------------------------

[
  "="
  ":="
  "+="
  "-="
  "*="
  "/="
  "%="
  "->"
  "=>"
  "?"
  ".."
] @operator

[
  "+"
  "-"
  "*"
  "/"
  "%"
  "=="
  "!="
  "<"
  ">"
  "<="
  ">="
  "&&"
  "||"
  "!"
  "&"
  "|"
  "^"
  "~"
  "<<"
  ">>"
] @operator

["(" ")" "[" "]" "{" "}"] @punctuation.bracket
["," ";" ":" "::" "."] @punctuation.delimiter

; ---- literals -------------------------------------------------------------

(boolean_literal) @boolean
(integer_literal) @number
(float_literal) @number.float
(string_literal) @string

; ---- keywords -------------------------------------------------------------

[
  "use"
  "fn"
  "extern"
  "struct"
  "enum"
  "impl"
  "test"
  "let"
  "as"
  "defer"
  "arena"
  "new"
] @keyword

[
  "if"
  "else"
  "while"
  "for"
  "in"
  "break"
  "continue"
  "return"
  "match"
] @keyword.control

(self_expression) @variable.builtin

[
  "true"
  "false"
] @boolean

; ---- builtins -------------------------------------------------------------

((identifier) @function.builtin
  (#any-of? @function.builtin
    "wlel_sizeof"
    "wlel_alloc"
    "wlel_free"
    "wlel_arena"
    "wlel_arena_new"
    "wlel_arena_alloc"
    "wlel_arena_free"
    "arena_stats"
    "panic"
    "assert"
    "assert_eq"))
