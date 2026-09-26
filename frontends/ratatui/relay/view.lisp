(defpackage :lem-relay/view
  (:use :cl)
  (:export :view
           :make-view
           :view-id
           :view-window
           :view-x
           :view-y
           :view-width
           :view-height
           :view-kind
           :view-modeline-p
           :view-border
           :view-border-shape))
(in-package :lem-relay/view)

(defstruct (view (:constructor make-view
                     (&key id window x y width height kind modeline-p
                        border border-shape)))
  "What the relay hands Lem from `lem-if:make-view' and gets back on every
later call about that window: the id the display knows it by, the Lem
WINDOW it shows, and its geometry in cells.

KIND is :tile, :header or :floating. WINDOW is NIL only in tests."
  id window x y width height kind modeline-p border border-shape)
