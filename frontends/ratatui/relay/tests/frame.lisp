(defpackage :lem-relay/tests/frame
  (:use :cl :rove :lem-relay/frame))
(in-package :lem-relay/tests/frame)

(defun put (view y text &key (style 0))
  (make-text-put :view view :x 0 :y y :text text :width (length text) :style style))

(defun finish (session &optional cursor)
  (finish-frame session cursor))

;;; Styles

(deftest equal-attributes-intern-to-one-id
  (let ((table (make-style-table)))
    ;; Lem builds a fresh attribute for every eol cursor and
    ;; extend-to-eol it draws, so identity cannot be the key.
    (flet ((red () (lem:make-attribute :foreground "#FF0000" :bold t)))
      (ok (= (intern-style table (attribute-style (red)))
             (intern-style table (attribute-style (red))))))
    (ng (= (intern-style table (attribute-style (lem:make-attribute :foreground "#FF0000")))
           (intern-style table (attribute-style (lem:make-attribute :foreground "#00FF00")))))))

(deftest colour-spellings-normalise
  (let ((by-string (attribute-style (lem:make-attribute :background "#102030")))
        (by-struct (attribute-style (lem:make-attribute
                                     :background (lem:make-color #x10 #x20 #x30)))))
    (ok (eql #x102030 (style-background by-string)))
    (ok (equalp by-string by-struct))))

(deftest the-default-style-is-zero
  (let ((table (make-style-table)))
    (ok (zerop (intern-style table (attribute-style nil))))
    (ok (= 1 (intern-style table (make-style :bold t))))
    (ok (null (style-by-id table 0)))))

(deftest a-coloured-underline-keeps-its-colour
  (ok (eql #xFF0000 (style-underline (attribute-style (lem:make-attribute :underline "#FF0000")))))
  (ok (eq t (style-underline (attribute-style (lem:make-attribute :underline t))))))

;;; Frame suppression

(deftest a-frame-with-nothing-in-it-is-not-sent
  (let ((session (make-session)))
    (ok (null (finish session)))
    (ok (= 1 (session-dropped-count session)))))

(deftest reblanking-a-blank-region-is-not-sent
  (let ((session (make-session)))
    (add-op session (make-rest-cleared :view 1 :y 5))
    (ok (finish session) "the first blank is news")
    (add-op session (make-rest-cleared :view 1 :y 5))
    (ok (null (finish session)) "the same blank again is not")
    (add-op session (make-rest-cleared :view 1 :y 3))
    (ok (finish session) "blanking from another row is")))

(deftest painting-a-view-makes-its-blank-region-stale
  (let ((session (make-session)))
    (add-op session (make-rest-cleared :view 1 :y 5))
    (finish session)
    ;; Lem draws lines first and blanks what is left after, in one frame:
    ;; the blank at the end of this frame must still be remembered.
    (add-op session (put 1 0 "abc"))
    (add-op session (make-rest-cleared :view 1 :y 5))
    (ok (finish session))
    (add-op session (make-rest-cleared :view 1 :y 5))
    (ok (null (finish session)) "remembered despite the paint before it")
    (add-op session (put 2 0 "other view"))
    (finish session)
    (add-op session (make-rest-cleared :view 1 :y 5))
    (ok (null (finish session)) "painting another view changes nothing here")))

(deftest a-cursor-that-did-not-move-is-not-sent
  (let ((session (make-session)))
    (ok (finish session (make-cursor :view 1 :x 3 :y 4)))
    (ok (null (finish session (make-cursor :view 1 :x 3 :y 4))))
    (ok (finish session (make-cursor :view 1 :x 4 :y 4)))))

(deftest frames-are-numbered-only-when-sent
  (let ((session (make-session)))
    (add-op session (put 1 0 "a"))
    (ok (= 1 (frame-seq (finish session))))
    (finish session)
    (add-op session (put 1 0 "b"))
    (ok (= 2 (frame-seq (finish session))))))

(deftest a-frame-keeps-its-ops-in-order
  (let ((session (make-session))
        (a (put 1 0 "a"))
        (b (make-line-cleared :view 1 :x 1 :y 0))
        (c (make-rest-cleared :view 1 :y 1)))
    (add-op session a)
    (add-op session b)
    (add-op session c)
    (ok (equal (list a b c) (frame-ops (finish session))))))

;;; Modeline memoisation

(defun modeline (view text)
  (make-modeline-painted :view view :puts (list (put view 0 text))))

(deftest an-unchanged-modeline-is-not-resent
  (let ((session (make-session)))
    (ok (add-op session (modeline 1 "main.lisp  L1")))
    (finish session)
    (ok (null (add-op session (modeline 1 "main.lisp  L1"))))
    (ok (null (finish session)) "and the frame it was alone in is dropped")
    (ok (add-op session (modeline 1 "main.lisp  L2")))))

(deftest a-modeline-is-resent-after-its-view-changes
  (dolist (change (list (make-view-resized :view 1 :width 40 :height 10)
                        (make-view-created :view 1 :x 0 :y 0 :width 80 :height 23
                                           :kind :tile :modeline-p t)
                        (make-view-deleted :view 1)))
    (let ((session (make-session)))
      (add-op session (modeline 1 "same"))
      (finish session)
      (add-op session change)
      (ok (add-op session (modeline 1 "same"))
          (format nil "after ~(~A~)" (type-of change))))))

;;; Styles on the wire

(deftest a-style-is-defined-once-on-the-wire
  (let* ((session (make-session))
         (bold (intern-style (session-styles session) (make-style :bold t))))
    (add-op session (put 1 0 "x" :style bold))
    (add-op session (put 1 1 "y" :style bold))
    (let ((frame (finish session)))
      (ok (equal (list bold) (mapcar #'car (frame-new-styles frame))))
      (ok (style-bold (cdr (first (frame-new-styles frame))))))
    (add-op session (put 1 2 "z" :style bold))
    (ok (null (frame-new-styles (finish session))) "already defined")))

(deftest the-default-style-is-never-defined
  (let ((session (make-session)))
    (add-op session (put 1 0 "plain"))
    (ok (null (frame-new-styles (finish session))))))

(deftest a-style-first-used-in-a-dropped-modeline-is-still-defined-later
  ;; A style can be interned while drawing something that is then not
  ;; sent. It must be defined in the first frame that actually uses it.
  (let* ((session (make-session))
         (id (intern-style (session-styles session) (make-style :reverse t))))
    (ok (null (finish session)))
    (add-op session (put 1 0 "now" :style id))
    (ok (equal (list id) (mapcar #'car (frame-new-styles (finish session)))))))
