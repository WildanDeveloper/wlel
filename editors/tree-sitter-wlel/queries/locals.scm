; Scope and definition tracking for Wlel.
;
; Wlel has block scoping and no closures, so a scope tree is enough: locals
; defined in a block are visible until that block closes, and `locals.scm`
; lets editors resolve references, rename and highlight usages.

; ---- scopes ---------------------------------------------------------------

(function_declaration) @scope
(extern_declaration) @scope
(block) @scope
(block_expression) @scope
(match_arm) @scope
(for_statement) @scope

; ---- definitions ----------------------------------------------------------

(function_declaration
  name: (identifier) @definition.function)

(extern_declaration
  name: (identifier) @definition.function)

(struct_declaration
  name: (identifier) @definition.type)

(enum_declaration
  name: (identifier) @definition.type)

(variant
  name: (identifier) @definition.enumMember)

(parameter
  name: (identifier) @definition.parameter)

(let_declaration
  name: (identifier) @definition.var)

(short_declaration
  name: (identifier) @definition.var)

(for_statement
  variable: (identifier) @definition.var)

(variant_pattern
  variant: (identifier) @definition.enumMember)

(variant_pattern
  (identifier) @definition.var)

(pattern
  (identifier) @definition.var)

; ---- references -----------------------------------------------------------

(identifier) @reference

; ---- builtins -------------------------------------------------------------

((identifier) @definition.builtin
  (#any-of? @definition.builtin
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
