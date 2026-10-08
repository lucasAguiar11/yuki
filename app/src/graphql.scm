; Highlights for tree-sitter-graphql 0.1 (gpui-component registers the grammar
; with an empty query). First match wins: specific patterns come first.

(comment) @comment
(description) @comment

(variable) @variable.special
(named_type (name) @type)
(operation_definition (name) @function)
(fragment_name (name) @function)
(directive (name) @attribute)
"@" @attribute

(alias (name) @property)
(field (name) @property)
(argument (name) @property)
(object_field (name) @property)

(string_value) @string
(int_value) @number
(float_value) @number
(boolean_value) @boolean
(null_value) @constant
(enum_value) @constant

[
  "query"
  "mutation"
  "subscription"
  "fragment"
  "on"
  "type"
  "input"
  "enum"
  "interface"
  "union"
  "scalar"
  "schema"
  "extend"
  "implements"
  "directive"
  "repeatable"
] @keyword

["{" "}" "(" ")" "[" "]"] @punctuation.bracket
[":" "=" "!" "|" "&" "..."] @punctuation.delimiter
