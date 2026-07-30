// Scenario fidelity tests: Data Structure Editing
//
// Multi-step workflows for manipulating data structures.

// ═══════════════════════════════════════════════════════════════════════════════
// ARRAY / LIST EDITING
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, array_append_item, "[1, 2, 3]", cursor(0, 7), "a, 4<Esc>");
neovim_test!(scenarios, array_prepend_item, "[1, 2, 3]", cursor(0, 1), "i0, <Esc>");
neovim_test!(scenarios, array_delete_item, "[1, 2, 3]", cursor(0, 4), "dt,");
neovim_test!(scenarios, array_change_item, "[1, 2, 3]", cursor(0, 4), "r9");
neovim_test!(scenarios, array_clear_contents, "[a, b, c]", cursor(0, 1), "di[");
neovim_test!(scenarios, array_delete_with_brackets, "x = [a, b, c]", cursor(0, 5), "da[");

// ═══════════════════════════════════════════════════════════════════════════════
// OBJECT / STRUCT FIELDS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, change_field_value, "name: \"old\"", cursor(0, 7), "ci\"new<Esc>");
neovim_test!(scenarios, change_field_name, "old_name: value", "ciwnew_name<Esc>");
neovim_test!(scenarios, add_field_after, "name: \"Bob\"", "A,<CR>age: 30<Esc>");
neovim_test!(scenarios, delete_field_line, "field1: 1\nfield2: 2\nfield3: 3", cursor(1, 0), "dd");

// ═══════════════════════════════════════════════════════════════════════════════
// JSON-LIKE EDITING
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, json_change_value, "{\"key\": \"old\"}", cursor(0, 8), "ci\"new<Esc>");
neovim_test!(scenarios, json_change_key, "{\"old_key\": 42}", cursor(0, 2), "ci\"new_key<Esc>");
neovim_test!(scenarios, json_add_field, "{\"a\": 1}", cursor(0, 6), "a, \"b\": 2<Esc>");
neovim_test!(scenarios, json_delete_value, "{\"key\": \"remove\"}", cursor(0, 8), "di\"");
neovim_test!(scenarios, json_change_number, "{\"count\": 42}", cursor(0, 10), "ciw99<Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// STRUCT / ENUM EDITING
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, struct_add_field, "struct Foo {}", cursor(0, 12), "i\n    name: String,\n<Esc>");
neovim_test!(scenarios, struct_change_type, "    age: u32,", cursor(0, 9), "ciwu64<Esc>");
neovim_test!(scenarios, enum_add_variant, "enum Color { Red }", cursor(0, 16), "i, Blue<Esc>");
neovim_test!(scenarios, enum_rename_variant, "enum Dir { Up }", cursor(0, 11), "ciwNorth<Esc>");
neovim_test!(scenarios, add_derive, "struct Config {}", "O#[derive(Debug, Clone)]<Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// TOML / CONFIG FILES
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, toml_change_version, "version = \"0.1.0\"", cursor(0, 11), "ci\"0.2.0<Esc>");
neovim_test!(scenarios, toml_add_dep, "[dependencies]", "orand = \"0.8\"<Esc>");
neovim_test!(scenarios, toml_toggle_feature, "feature = false", "ffc5ltrue<Esc>");
neovim_test!(scenarios, ini_change_value, "host = localhost", "f=lcwremote.server<Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// YAML-LIKE
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, yaml_change_value, "  port: 8080", "f8cw3000<Esc>");
neovim_test!(scenarios, yaml_add_field, "  name: test", "o  debug: true<Esc>");
neovim_test!(scenarios, yaml_delete_field, "a: 1\nb: 2\nc: 3", cursor(1, 0), "dd");

// ═══════════════════════════════════════════════════════════════════════════════
// NESTED STRUCTURES
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, nested_array, "[[1, 2], [3, 4]]", cursor(0, 1), "di[");
neovim_test!(scenarios, nested_obj_field, "{a: {b: old}}", cursor(0, 8), "ciwupdated<Esc>");
neovim_test!(scenarios, nested_change_inner, "fn(a(b))", cursor(0, 4), "ci(X<Esc>");
neovim_test!(scenarios, deep_bracket, "[[[core]]]", cursor(0, 2), "di[");

// ═══════════════════════════════════════════════════════════════════════════════
// DUPLICATE AND MODIFY
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, dup_struct_field, "    width: f64,", "yypwcwheight<Esc>");
neovim_test!(scenarios, dup_array_entry, "items = [\"alpha\"]", "f]i, \"beta\"<Esc>");
neovim_test!(scenarios, dup_json_entry, "  \"name\": \"old\"", "yypf:lcwi\"new\"<Esc>");

