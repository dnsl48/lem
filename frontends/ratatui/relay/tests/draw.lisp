(defpackage :lem-relay/tests/draw
  (:use :cl :rove :lem-relay/frame :lem-relay/view :lem-relay/draw)
  (:local-nicknames (:display :lem-core/display)))
(in-package :lem-relay/tests/draw)

(defun a-view (&key (width 20) (height 5))
  (make-view :id 7 :x 0 :y 0 :width width :height height :kind :tile))

(defun text (string &optional attribute (class 'display:text-object))
  (make-instance class :string string :attribute attribute :type nil))

(defun queued (session)
  "The ops SESSION has queued, in order, closing the frame to get them."
  (frame-ops (finish-frame session nil)))

;;; Single objects

(deftest a-text-object-is-one-put
  (let ((puts (object-puts (text "defun" (lem:make-attribute :foreground "#AABBCC"))
                           3 1 (a-view))))
    (ok (= 1 (length puts)))
    (let ((put (first puts)))
      (ok (equal "defun" (text-put-text put)))
      (ok (= 7 (text-put-view put)))
      (ok (= 3 (text-put-x put)))
      (ok (= 5 (text-put-width put)))
      (ok (eql #xAABBCC (style-foreground (text-put-style put)))))))

(deftest widths-are-cells-not-characters
  ;; The relay's width is what the display will fit the run to (ADR 0012).
  (ok (= 4 (object-width (text "日本"))))
  (ok (= 4 (text-put-width (first (object-puts (text "日本") 0 0 (a-view)))))))

(deftest icons-and-emoji-are-plain-text
  (dolist (class '(display:icon-object display:emoji-object display:folder-object))
    (ok (equal "x" (text-put-text (first (object-puts (text "x" nil class) 0 0 (a-view)))))
        (string-downcase (symbol-name class)))))

(deftest an-attribute-name-is-resolved
  ;; Named with a T spec: light/dark specs ask the running implementation
  ;; which theme mode is on, and a test has none.
  (let ((put (first (object-puts (text "x" 'lem:document-bold-attribute) 0 0 (a-view)))))
    (ok (style-bold (text-put-style put)))))

(deftest line-end-text-sits-at-its-offset
  (let ((put (first (object-puts (make-instance 'display:line-end-object
                                                :string "<-" :attribute nil :type nil
                                                :offset 2)
                                 10 0 (a-view)))))
    (ok (= 12 (text-put-x put)))))

(deftest an-eol-cursor-is-a-cursor-cell
  (let ((put (first (object-puts (make-instance 'display:eol-cursor-object
                                                :color (lem:make-color #x11 #x22 #x33))
                                 4 0 (a-view)))))
    (ok (equal " " (text-put-text put)))
    (ok (eql #x112233 (style-background (text-put-style put))))
    (ok (style-cursor (text-put-style put)))))

(deftest extend-to-eol-fills-the-rest-of-the-row
  (let* ((object (make-instance 'display:extend-to-eol-object
                                :color (lem:make-color #x10 #x10 #x10)))
         (put (first (object-puts object 15 0 (a-view :width 20)))))
    (ok (= 5 (text-put-width put)))
    (ok (= 5 (length (text-put-text put))))
    (ok (null (object-puts object 20 0 (a-view :width 20))) "nothing when already at the edge")))

(deftest void-and-image-objects-draw-nothing
  (ok (null (object-puts (make-instance 'display:void-object) 0 0 (a-view))))
  (ok (null (object-puts (make-instance 'display:image-object) 0 0 (a-view))))
  (ok (zerop (object-width (make-instance 'display:image-object)))))

(defclass not-a-lem-object () ()
  (:documentation "A drawing object the relay has no method for. Lem's own
base class, drawing-object, is not exported, but the catch-all dispatches
on T, so this proves the same thing."))

(deftest an-unknown-object-is-skipped-not-fatal
  (ok (null (object-puts (make-instance 'not-a-lem-object) 0 0 (a-view)))))

;;; The cursor

(defun cursor-here-p (object)
  (nth-value 1 (object-puts object 0 0 (a-view))))

(deftest text-drawn-in-the-cursor-attribute-is-the-cursor
  (let ((attribute (lem:make-attribute :background "#FFFFFF")))
    (lem:set-cursor-attribute attribute)
    (ok (cursor-here-p (text "a" attribute)))
    (ng (cursor-here-p (text "a" (lem:make-attribute :background "#FFFFFF"))))))

;;; ncurses' corrections

(deftest only-the-true-eol-cursor-is-the-cursor
  ;; With multiple cursors, the others are eol-cursor-objects too.
  (flet ((eol-cursor (true-p)
           (make-instance 'display:eol-cursor-object
                          :color (lem:make-color 0 0 0) :true-cursor-p true-p)))
    (ok (cursor-here-p (eol-cursor t)))
    (ng (cursor-here-p (eol-cursor nil)))
    (ok (style-cursor (text-put-style (first (object-puts (eol-cursor nil) 0 0 (a-view)))))
        "but every one is still painted as a cursor")))

(deftest text-takes-the-drawing-windows-background
  (let ((lem-if:*background-color-of-drawing-window* "#202020"))
    (let ((plain (first (object-puts (text "a") 0 0 (a-view))))
          (coloured (first (object-puts (text "b" (lem:make-attribute :foreground "#FF0000"))
                                        0 0 (a-view))))
          (own (first (object-puts (text "c" (lem:make-attribute :background "#0000FF"))
                                   0 0 (a-view)))))
      (ok (eql #x202020 (style-background (text-put-style plain))) "no attribute")
      (ok (eql #x202020 (style-background (text-put-style coloured))) "no background")
      (ok (eql #xFF0000 (style-foreground (text-put-style coloured))) "keeping its colour")
      (ok (eql #x0000FF (style-background (text-put-style own))) "its own background wins"))))

(deftest outside-a-coloured-window-no-background-is-invented
  (ok (null (text-put-style (first (object-puts (text "a") 0 0 (a-view)))))))

(deftest clearing-below-the-bottom-is-skipped
  (let ((session (make-session)))
    (clear-to-end-of-window session (a-view :height 5) 5)
    (clear-to-end-of-window session (a-view :height 5) 3)
    (let ((ops (queued session)))
      (ok (= 1 (length ops)))
      (ok (= 3 (rest-cleared-y (first ops)))))))

;;; Lines and modelines

(deftest a-line-is-cleared-then-drawn-left-to-right
  (let ((session (make-session)))
    (render-line session (a-view) 2 1 (list (text "ab") (text "cde")))
    (let ((ops (queued session)))
      (ok (typep (first ops) 'line-cleared))
      (ok (= 2 (line-cleared-x (first ops))))
      (ok (equal '(2 4) (mapcar #'text-put-x (rest ops))))
      (ok (equal '("ab" "cde") (mapcar #'text-put-text (rest ops)))))))

(deftest a-modeline-is-one-op-blanked-then-left-then-right
  (let ((session (make-session)))
    (render-line-on-modeline session (a-view :width 20)
                             (list (text "file.lisp"))
                             (list (text "L1") (text "All "))
                             (lem:make-attribute :background "#404040"))
    (let* ((ops (queued session))
           (puts (modeline-painted-puts (first ops))))
      (ok (= 1 (length ops)))
      (ok (= 20 (text-put-width (first puts))) "blanked across the width")
      (ok (eql #x404040 (style-background (text-put-style (first puts)))))
      (ok (equal '(0 0 18 14) (mapcar #'text-put-x puts)) "right side laid out leftwards"))))
