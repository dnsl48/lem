(defpackage :lem-ratatui/font-styles
  (:use :cl)
  (:export :*attribute-font-styles*))
(in-package :lem-ratatui/font-styles)

;;; So an init.lisp shared with other frontends can say #+lem-ratatui.
(pushnew :lem-ratatui *features*)

(defvar *attribute-font-styles*
  '((lem:document-italic-attribute :italic))
  "Which Lem attributes this frontend draws italic, struck through or dim
(ADR 0015): a list of (ATTRIBUTE-NAME KEY...), each KEY one of :italic,
:strikethrough or :dim.

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

(defun apply-font-styles ()
  "Mark `*attribute-font-styles*' on the attributes the new theme built,
and repaint. Lem's line cache compares attributes without their property
lists, so lines already on screen would otherwise keep their old look."
  (when (plusp (lem-relay/frame:mark-font-styles *attribute-font-styles*))
    (lem:redraw-display :force t)))

(lem:add-hook lem:*after-load-theme-hook* 'apply-font-styles)
