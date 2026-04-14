; Perl tree-sitter highlight query for the ganezdragon/tree-sitter-perl
; grammar. Hand-written to match the grammar's named nodes, using
; capture names that match the rest of arx-highlight's theme.

; --- Comments ---
(comments) @comment
(pod_statement) @comment

; --- Strings ---
(string_single_quoted) @string
(string_double_quoted) @string
(string_q_quoted) @string
(string_qq_quoted) @string
(command_qx_quoted) @string
(backtick_quoted) @string
(word_list_qw) @string
(heredoc_initializer) @string
(heredoc_body_statement) @string
(escape_sequence) @string.escape

; --- Regex ---
(regex_pattern_qr) @string
(regex_pattern_content) @string
(pattern_matcher) @string
(pattern_matcher_m) @string
(substitution_pattern_s) @string
(transliteration_tr_or_y) @string
(regex_option) @string.escape
(regex_option_for_substitution) @string.escape
(regex_option_for_transliteration) @string.escape

; --- Numbers ---
(integer) @number
(floating_point) @number
(scientific_notation) @number
(hexadecimal) @number
(octal) @number

; --- Booleans / special literals ---
(true) @constant.builtin
(false) @constant.builtin
(special_literal) @constant.builtin
"constant" @constant

; --- Variables ---
(scalar_variable) @variable
(array_variable) @variable
(hash_variable) @variable
(package_variable) @variable
(special_scalar_variable) @variable.builtin
(typeglob) @variable
(file_handle) @variable.builtin
(standard_input) @variable.builtin

; --- Function calls ---
; Only `call_expression_with_bareword` and `method_invocation` have
; named `function_name` fields in this grammar; the other call
; expressions have the function name as a child identifier.
(call_expression_with_bareword
  function_name: (identifier) @function)

; --- Function definitions ---
(function_definition
  name: (identifier) @function)
(function_definition_without_sub
  name: (identifier) @function)

; --- Method calls ---
(method_invocation
  function_name: (identifier) @function.method)

; --- Packages / modules ---
(package_name) @namespace
(module_name) @namespace
"package" @keyword

; --- Use / require / no (these are anonymous tokens) ---
"use" @keyword
"no" @keyword
"require" @keyword
"import" @keyword

; --- Control flow ---
"if" @keyword
"elsif" @keyword
"else" @keyword
"unless" @keyword
"while" @keyword
"until" @keyword
"for" @keyword
"foreach" @keyword
"when" @keyword
"return" @keyword.return
"last" @keyword
"next" @keyword
"redo" @keyword
"goto" @keyword
"continue" @keyword

; --- Declarations ---
"my" @keyword
"our" @keyword
"local" @keyword
"state" @keyword
"sub" @keyword.function
"bless" @keyword

; --- Logical operators ---
"and" @keyword.operator
"or" @keyword.operator
"not" @keyword.operator
"xor" @keyword.operator
"eq" @keyword.operator
"ne" @keyword.operator
"lt" @keyword.operator
"le" @keyword.operator
"gt" @keyword.operator
"ge" @keyword.operator
"cmp" @keyword.operator
"isa" @keyword.operator

; --- Operators (single chars / punctuation) ---
[
  "="
  "+="
  "-="
  "*="
  "/="
  "%="
  "**="
  ".="
  "//="
  "||="
  "&&="
  "|="
  "&="
  "^="
  "<<="
  ">>="
  "+"
  "-"
  "*"
  "/"
  "%"
  "**"
  "."
  "=="
  "!="
  "<"
  ">"
  "<="
  ">="
  "<=>"
  "&&"
  "||"
  "//"
  "!"
  "?"
  ":"
  "->"
  "=~"
  "!~"
  ".."
  "..."
] @operator

; --- Punctuation ---
[
  "("
  ")"
  "["
  "]"
  "{"
  "}"
] @punctuation.bracket

[
  ","
  ";"
] @punctuation.delimiter

(fat_comma) @punctuation.special
(arrow_operator) @operator

; --- Labels ---
(label) @label

; --- Function attributes / prototypes ---
(function_attribute) @attribute
(function_prototype) @attribute
(prototype) @attribute

; --- Special blocks (BEGIN, END, etc.) ---
(special_block) @function.builtin

; --- Versions ---
(version) @number
