(uiop:define-package :lem-ratatui/implementation
  (:use :cl)
  (:export :ratatui))
(in-package :lem-ratatui/implementation)

(defclass ratatui (lem-server:jsonrpc lem-core:implementation)
  ()
  (:default-initargs
   :name :ratatui
   :redraw-after-modifying-floating-window t
   ;; A terminal has a single cell grid and composites nothing, so a
   ;; dismissed floating window leaves a hole that nothing repaints.
   ;; lem-server sets this to T because each browser view is its own
   ;; canvas and the windows underneath are still painted; here the
   ;; windows underneath must actually be re-sent.
   :no-force-needed nil
   ;; Cells, not pixels. The display half ignores pixelX/pixelY and
   ;; friends, so there is no reason for the core to compute them.
   :support-pixel-positioning nil
   ;; No html-buffer rendering in a terminal.
   :html-support nil
   ;; Reserve a column/row for window separators, as ncurses does.
   ;; lem-server uses 0 because the browser draws borders as overlay
   ;; elements rather than occupying cells.
   :window-left-margin 1
   :window-bottom-margin 1
   ;; Off, matching ncurses, despite modern terminals supporting SGR 58
   ;; and crossterm exposing SetUnderlineColor.
   ;;
   ;; The flag has exactly one call site in Lem
   ;; (src/ext/multi-column-list.lisp:214), and there it reaches for
   ;; `foreground-color' — the *theme's* foreground, which `lem-default'
   ;; deliberately leaves NIL so the frontend can supply its own. That
   ;; NIL goes straight into `darken-color' and C-x C-b dies with "The
   ;; value NIL is not of type LEM/COMMON/COLOR:COLOR".
   ;;
   ;; Turning it off costs no rendering: an attribute that names an
   ;; underline colour still gets one, because the display half maps
   ;; `Underline::Color' regardless of this flag. It only stops Lem
   ;; taking the branch that crashes.
   :underline-color-support nil)
  (:documentation "Frontend implementation for the Rust/Ratatui display process.

Inherits the whole `lem-if:*' protocol from `lem-server:jsonrpc' and only
overrides the capability flags where a terminal differs from a browser.

`lem-core:implementation' is listed as an explicit superclass because
`lem-core:get-default-implementation' selects a frontend by scanning
`c2mop:class-direct-subclasses' of `implementation' and comparing class
names against the --interface argument. Inheriting solely through
`lem-server:jsonrpc' would make this class an indirect subclass and
therefore invisible to that lookup. `lem-webview:webview' uses the same
idiom for the same reason."))
