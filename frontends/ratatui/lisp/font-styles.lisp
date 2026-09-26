(defpackage :lem-ratatui/font-styles
  (:use :cl)
  (:export :*attribute-font-styles*))
(in-package :lem-ratatui/font-styles)

;;; So an init.lisp shared with other frontends can say #+lem-ratatui.
(pushnew :lem-ratatui *features*)

(defvar *attribute-font-styles*
  '((lem:document-italic-attribute :italic)
    (lem:compiler-note-attribute (:underline-style :curly)))
  "Which Lem attributes this frontend draws italic, struck through or dim
(ADR 0015, 0016): a list of (ATTRIBUTE-NAME STYLE...), each STYLE one of
:italic, :strikethrough, :dim, or (:underline-style S) with S one of
:curly, :dotted, :dashed or :double.

Lem's attributes have no slots for these, so the keys go in each
attribute's property list, which Lem keeps through merging and theme
loads. By default only `document-italic-attribute' (Markdown emphasis,
tree-sitter's markup.italic), the one attribute that exists for a font
style. In init.lisp, for italic comments:

  #+lem-ratatui
  (push '(lem:syntax-comment-attribute :italic)
        lem-ratatui/font-styles:*attribute-font-styles*)

`#+lem-ratatui' keeps other frontends from reading a package they do
not have. An attribute defined with `:plist '(:italic t)' needs no entry
here at all.")

(defparameter *lsp-diagnostic-attributes*
  '("DIAGNOSTIC-ERROR-ATTRIBUTE" "DIAGNOSTIC-WARNING-ATTRIBUTE"
    "DIAGNOSTIC-INFORMATION-ATTRIBUTE" "DIAGNOSTIC-HINT-ATTRIBUTE")
  "The attributes lsp-mode underlines diagnostics with
(extensions/lsp-mode/lsp-mode.lisp). They are not exported from
`lem-lsp-mode/lsp-mode', so they are found by name when the theme loads:
a build without lsp-mode, or a rename upstream, skips them rather than
failing to load.")

(defun lsp-diagnostic-entries ()
  "Curly underlines for LSP diagnostics, for the attributes that exist."
  (alexandria:when-let ((package (find-package :lem-lsp-mode/lsp-mode)))
    (loop :for name :in *lsp-diagnostic-attributes*
          :for symbol := (find-symbol name package)
          :when symbol :collect (list symbol '(:underline-style :curly)))))

(defun apply-font-styles ()
  "Mark `*attribute-font-styles*', and LSP diagnostics curly, on the
attributes the new theme built, and repaint. Lem's line cache compares
attributes without their property lists, so lines already on screen
would otherwise keep their old look."
  (when (plusp (lem-relay/frame:mark-font-styles
                (append *attribute-font-styles* (lsp-diagnostic-entries))))
    (lem:redraw-display :force t)))

(lem:add-hook lem:*after-load-theme-hook* 'apply-font-styles)
