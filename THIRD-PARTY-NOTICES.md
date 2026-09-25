# Third-party notices

The GDScript syntax frontend lives in `crates/gdview/src/syntax` (ported from the
standalone gdview crate at the revision below). Its syntax kinds, grammar productions, and indentation
handling are adapted from gdscript-syntax in reactive-ui-toolkit/gdscript-analyzer.

Dependency: https://github.com/xandwr/gdview
Revision: 49df9290891f9bbf5708eabbc59f56dfc1cf263d

Source: https://github.com/reactive-ui-toolkit/gdscript-analyzer
Revision: f5f70e1c35e1eff93658a4f3e8de01b889bbfee0
License selected: MIT. The original notice is in
[licenses/gdscript-syntax-MIT.txt](licenses/gdscript-syntax-MIT.txt).

Adaptations maintained in gdview replace the lexer and tree implementation, generate token
metadata and typed AST views with declarative macros, and modify parsing and
recovery behavior. Upstream code remains subject to its retained MIT notice.
