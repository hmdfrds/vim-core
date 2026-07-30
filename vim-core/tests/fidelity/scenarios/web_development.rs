// Scenario fidelity tests: Web Development Workflows
//
// Realistic editing patterns a web developer encounters daily:
// Python backends, JS/TS frontends, HTML templates, CSS styling,
// JSON configs, Markdown docs, and common mistake-fixing flows.

// ═══════════════════════════════════════════════════════════════════════════════
// PYTHON: INDENTATION AND DECORATORS
// ═══════════════════════════════════════════════════════════════════════════════

// Fix a Flask route handler that was pasted at wrong indent level
neovim_test!(scenarios, py_fix_indent_flask_handler,
    "class Api:\n    def index(self):\nreturn jsonify(data)",
    cursor(2, 0), ">>>>>>>");

// Outdent a block that was over-indented during refactor
neovim_test!(scenarios, py_outdent_overindented_block,
    "def run():\n            result = compute()\n            return result",
    cursor(1, 0), "<<j<<");

// Add @login_required decorator above a Django view
neovim_test!(scenarios, py_add_login_required_decorator,
    "def dashboard(request):\n    return render(request, 'dash.html')",
    "O@login_required<Esc>");

// Add @staticmethod then fix the self parameter
neovim_test!(scenarios, py_staticmethod_remove_self,
    "class Util:\n    def helper(self, data):\n        return data",
    cursor(1, 0), "O    @staticmethod<Esc>j/self, <CR>dn");

// Edit a Python dict: change a key's value
neovim_test!(scenarios, py_change_dict_value,
    "config = {\n    'host': 'localhost',\n    'port': 8080,\n}",
    cursor(1, 0), "f'lci'0.0.0.0<Esc>");

// Add a new key-value pair to a Python dict
neovim_test!(scenarios, py_add_dict_entry,
    "config = {\n    'debug': True,\n}",
    cursor(1, 0), "o    'verbose': False,<Esc>");

// Change a list comprehension filter
neovim_test!(scenarios, py_edit_list_comprehension,
    "valid = [x for x in items if x > 0]",
    cursor(0, 31), "ciw10<Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// JAVASCRIPT: ARROW FUNCTIONS AND TEMPLATE LITERALS
// ═══════════════════════════════════════════════════════════════════════════════

// Convert function declaration to arrow function assigned to const
neovim_test!(scenarios, js_func_to_const_arrow,
    "function handleClick(e) {",
    "ciwconst<Esc>ea = <Esc>f{i=> <Esc>");

// Change a regular string to a template literal by replacing quotes
// and inserting an interpolation
neovim_test!(scenarios, js_add_template_interpolation,
    "const msg = \"Hello, \" + name + \"!\";",
    cursor(0, 13), "r`f\"r`");

// Add async/await to a fetch call
neovim_test!(scenarios, js_add_async_await_fetch,
    "const data = fetch('/api/users');",
    "^ciwconst<Esc>f=la await<Esc>");

// Destructure an object parameter in a function
neovim_test!(scenarios, js_destructure_param,
    "function render(props) {",
    cursor(0, 16), "ci({ name, age }<Esc>");

// Add optional chaining to a property access
neovim_test!(scenarios, js_add_optional_chaining,
    "const city = user.address.city;",
    cursor(0, 17), "a?<Esc>f.i?<Esc>");

// Change a require() to an import statement
neovim_test!(scenarios, js_require_to_import,
    "const express = require('express');",
    "ciwimport<Esc>f=cfwfrom<Esc>f;x$x");

// Add a catch clause to a promise chain
neovim_test!(scenarios, js_add_catch_to_promise,
    "fetch('/api')\n  .then(res => res.json())",
    cursor(1, 0), "o  .catch(err => console.error(err))<Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// HTML: TAGS AND ATTRIBUTES
// ═══════════════════════════════════════════════════════════════════════════════

// Change the content inside an HTML tag
neovim_test!(scenarios, html_change_tag_content,
    "<h1>Old Title</h1>",
    cursor(0, 4), "cit New Page Title<Esc>");

// Delete everything inside a tag (clear a div)
neovim_test!(scenarios, html_clear_div_contents,
    "<div>remove all this content</div>",
    cursor(0, 5), "dit");

// Add an id attribute to an existing tag
neovim_test!(scenarios, html_add_id_attribute,
    "<section class=\"hero\">\n  <h1>Welcome</h1>\n</section>",
    cursor(0, 8), "hi id=\"main-hero\"<Esc>");

// Change an href attribute value
neovim_test!(scenarios, html_change_href,
    "<a href=\"/old-page\">Click here</a>",
    cursor(0, 9), "ci\"/new-page<Esc>");

// Wrap text in a new tag by yanking and surrounding
neovim_test!(scenarios, html_wrap_text_in_strong,
    "<p>important text</p>",
    cursor(0, 3), "citimportant <strong>text</strong><Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// CSS: CHANGING VALUES AND DUPLICATING RULES
// ═══════════════════════════════════════════════════════════════════════════════

// Change a pixel value in a CSS property
neovim_test!(scenarios, css_change_px_value,
    "  margin: 16px;",
    cursor(0, 10), "ciw24px<Esc>");

// Change a color hex code
neovim_test!(scenarios, css_change_hex_color,
    "  color: #ff0000;",
    cursor(0, 9), "cW#3b82f6;<Esc>");

// Duplicate a CSS rule and change the selector
neovim_test!(scenarios, css_duplicate_rule_change_selector,
    ".header {\n  display: flex;\n}",
    "Vjjyp0ciw.footer<Esc>");

// Add a new property to a CSS rule
neovim_test!(scenarios, css_add_property_to_rule,
    ".container {\n  width: 100%;\n}",
    cursor(1, 0), "o  max-width: 1200px;<Esc>");

// Change a CSS class name with cW (WORD-wise to grab the whole selector)
neovim_test!(scenarios, css_rename_class,
    ".old-button:hover {\n  opacity: 0.8;\n}",
    cursor(0, 1), "ciwold-component<Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// JSON: KEYS, REORDERING, COMMAS
// ═══════════════════════════════════════════════════════════════════════════════

// Add a new field to a JSON object
neovim_test!(scenarios, json_add_field_to_package,
    "{\n  \"name\": \"my-app\",\n  \"version\": \"1.0.0\"\n}",
    cursor(2, 0), "A,<CR>  \"description\": \"A web app\"<Esc>");

// Fix a trailing comma in JSON (invalid JSON)
neovim_test!(scenarios, json_remove_trailing_comma,
    "{\n  \"key\": \"value\",\n}",
    cursor(1, 0), "$hx");

// Change a JSON boolean value
neovim_test!(scenarios, json_toggle_boolean,
    "  \"private\": true",
    cursor(0, 13), "ciwfalse<Esc>");

// Reorder JSON fields by swapping lines
neovim_test!(scenarios, json_reorder_fields_swap,
    "{\n  \"b\": 2,\n  \"a\": 1,\n  \"c\": 3\n}",
    cursor(1, 0), "ddp");

// Change a nested JSON value deep in the object
neovim_test!(scenarios, json_change_nested_value,
    "  \"scripts\": {\n    \"dev\": \"vite\",\n    \"build\": \"tsc && vite build\"\n  }",
    cursor(1, 0), "f\"lf\"lci\"next dev<Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// MARKDOWN: HEADERS, LISTS, LINKS
// ═══════════════════════════════════════════════════════════════════════════════

// Change a markdown heading level from h2 to h3
neovim_test!(scenarios, md_h2_to_h3,
    "## Installation",
    "I#<Esc>");

// Convert a plain line into a markdown list item
neovim_test!(scenarios, md_convert_to_list_item,
    "Install dependencies\nRun the dev server\nOpen browser",
    "I- <Esc>j.j.");

// Edit the URL in a markdown link
neovim_test!(scenarios, md_change_link_url,
    "See [the docs](https://old-url.com) for details.",
    cursor(0, 15), "ci(https://new-url.com<Esc>");

// Change the link text in a markdown link
neovim_test!(scenarios, md_change_link_text,
    "Visit [old text](https://example.com).",
    cursor(0, 7), "ci[documentation<Esc>");

// Add a fenced code block annotation (change language)
neovim_test!(scenarios, md_change_codeblock_lang,
    "```javascript\nconst x = 1;\n```",
    cursor(0, 3), "Ctypescript<Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// REAL MISTAKES: TYPOS, WRONG INDENT, UNDO/REDO
// ═══════════════════════════════════════════════════════════════════════════════

// Fix "cosnt" typo - a classic JS developer mistake
neovim_test!(scenarios, webdev_fix_const_typo,
    "cosnt router = express.Router();",
    "ciwconst<Esc>");

// Fix "fucntion" typo with cw
neovim_test!(scenarios, webdev_fix_function_typo,
    "fucntion handleSubmit(e) {",
    "ciwfunction<Esc>");

// Fix wrong indent: a line was pasted at col 0 inside a function
neovim_test!(scenarios, webdev_fix_flat_paste_indent,
    "function init() {\nconst app = express();\n}",
    cursor(1, 0), ">>");

// Make an edit, realize it was wrong, undo, then redo differently
neovim_test!(scenarios, webdev_undo_redo_rethink,
    "const API_URL = '/api/v1';",
    cursor(0, 17), "ci'/api/v2<Esc>uci'/api/v3<Esc>");

// Delete a line, undo it immediately, then delete a different line
neovim_test!(scenarios, webdev_undo_delete_wrong_line,
    "import React from 'react';\nimport { useState } from 'react';\nimport axios from 'axios';",
    cursor(1, 0), "ddujdd");

// Type in insert mode, escape, realize more is needed, use A to append
neovim_test!(scenarios, webdev_append_after_insert,
    "const items = []",
    "A;<CR>const count = items.length;<Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// MULTI-CONTEXT PATTERNS: YANK IN ONE PLACE, PASTE IN ANOTHER
// ═══════════════════════════════════════════════════════════════════════════════

// Yank a CSS class name, navigate down, paste it into an HTML attribute
neovim_test!(scenarios, webdev_yank_classname_paste_in_html,
    ".sidebar-nav {\n  display: flex;\n}\n<div class=\"\">",
    cursor(0, 1), "yiwjjjf\"p");

// Yank a function name from definition, paste into a call site below
neovim_test!(scenarios, webdev_yank_funcname_paste_call,
    "function validateEmail(input) {\n  return true;\n}\nconst result = (data);",
    cursor(0, 9), "yiwjjjf(P");

// Copy an import path and paste it in another import
neovim_test!(scenarios, webdev_copy_import_path,
    "import { foo } from './utils/helpers';\nimport { bar } from '';",
    cursor(0, 20), "yi'jf'p");

// Yank a variable name with yiw and use it in a new line
neovim_test!(scenarios, webdev_yank_varname_use_below,
    "const userData = await fetchUser(id);\nconsole.log();",
    "yiwjo<Esc>\"0P");

// ═══════════════════════════════════════════════════════════════════════════════
// TYPESCRIPT-SPECIFIC PATTERNS
// ═══════════════════════════════════════════════════════════════════════════════

// Add a type annotation to a function parameter
neovim_test!(scenarios, ts_add_param_type,
    "function greet(name) {",
    cursor(0, 19), "i: string<Esc>");

// Change a type from string to number
neovim_test!(scenarios, ts_change_type_annotation,
    "let count: string = '0';",
    cursor(0, 11), "ciwnumber<Esc>f'ci'0<Esc>");

// Add generic type parameter to a function
neovim_test!(scenarios, ts_add_generic,
    "function identity(arg) {",
    cursor(0, 17), "i<T><Esc>f)i: T<Esc>");

// Add return type to an arrow function
neovim_test!(scenarios, ts_add_arrow_return_type,
    "const add = (a: number, b: number) => a + b;",
    cursor(0, 34), "i: number <Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// REACT / JSX PATTERNS
// ═══════════════════════════════════════════════════════════════════════════════

// Change a JSX component's prop value
neovim_test!(scenarios, jsx_change_prop_value,
    "  <Button size=\"small\" onClick={handleClick}>",
    cursor(0, 15), "ci\"large<Esc>");

// Delete a prop from a JSX component
neovim_test!(scenarios, jsx_delete_prop,
    "<Input type=\"text\" disabled />",
    cursor(0, 18), "daw");

// Add a className prop to a JSX element
neovim_test!(scenarios, jsx_add_classname,
    "<div>\n  <span>Hello</span>\n</div>",
    cursor(1, 7), "hi className=\"greeting\"<Esc>");
