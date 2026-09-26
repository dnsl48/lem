(defpackage :lem-relay/tests/frame
  (:use :cl :rove :lem-relay/frame))
(in-package :lem-relay/tests/frame)

(defun put (view y text &key style)
  (make-text-put :view view :x 0 :y y :text text :width (length text) :style style))

(defun finish (session &optional cursor)
  (finish-frame session cursor))

(defun settled-session (&rest args)
  "A session whose first frame, which always carries the default colours,
has already gone out."
  (let ((session (apply #'make-session args)))
    (finish session)
    session))

;;; Styles

(deftest equal-attributes-give-equal-styles
  ;; Lem builds a fresh attribute for every eol cursor and extend-to-eol
  ;; it draws, so styles must compare by content, not identity.
  (flet ((red () (lem:make-attribute :foreground "#FF0000" :bold t)))
    (ok (equalp (attribute-style (red)) (attribute-style (red)))))
  (ng (equalp (attribute-style (lem:make-attribute :foreground "#FF0000"))
              (attribute-style (lem:make-attribute :foreground "#00FF00")))))

(deftest colour-spellings-normalise
  (let ((by-string (attribute-style (lem:make-attribute :background "#102030")))
        (by-struct (attribute-style (lem:make-attribute
                                     :background (lem:make-color #x10 #x20 #x30)))))
    (ok (eql #x102030 (style-background by-string)))
    (ok (equalp by-string by-struct))))

(deftest no-attribute-is-no-style
  (ok (null (attribute-style nil))))

(deftest a-coloured-underline-keeps-its-colour
  (ok (eql #xFF0000 (style-underline (attribute-style (lem:make-attribute :underline "#FF0000")))))
  (ok (eq t (style-underline (attribute-style (lem:make-attribute :underline t))))))

;;; Frame suppression

(deftest a-frame-with-nothing-new-in-it-is-not-sent
  (let ((session (settled-session)))
    (ok (null (finish session)))
    (ok (= 1 (session-dropped-count session)))))

(deftest reblanking-a-blank-region-is-not-sent
  (let ((session (settled-session)))
    (add-op session (make-rest-cleared :view 1 :y 5))
    (ok (finish session) "the first blank is news")
    (add-op session (make-rest-cleared :view 1 :y 5))
    (ok (null (finish session)) "the same blank again is not")
    (add-op session (make-rest-cleared :view 1 :y 3))
    (ok (finish session) "blanking from another row is")))

(deftest painting-a-view-makes-its-blank-region-stale
  (let ((session (settled-session)))
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

(deftest a-frame-says-which-input-it-can-reflect
  (let ((session (settled-session)))
    (add-op session (put 1 0 "a"))
    (ok (= 41 (frame-input-seq (finish-frame session nil 41))))
    (add-op session (put 1 0 "b"))
    (ok (null (frame-input-seq (finish-frame session nil))) "none yet")))

(deftest frames-carry-the-relays-time
  (let* ((session (settled-session))
         (first (progn (add-op session (put 1 0 "a")) (finish session)))
         (second (progn (sleep 0.01) (add-op session (put 1 0 "b")) (finish session))))
    (ok (integerp (frame-time first)))
    (ok (< (frame-time first) (frame-time second)) "monotonic, in microseconds")))

(deftest a-frame-keeps-its-ops-in-order
  (let ((session (make-session))
        (a (put 1 0 "a"))
        (b (make-line-cleared :view 1 :x 1 :y 0))
        (c (make-rest-cleared :view 1 :y 1)))
    (add-op session a)
    (add-op session b)
    (add-op session c)
    (ok (equal (list a b c) (frame-ops (finish session))))))

;;; The cursor

(deftest a-cursor-that-did-not-change-is-not-sent
  (let ((session (settled-session)))
    (ok (finish session (make-cursor :view 1 :x 3 :y 4)))
    (ok (null (finish session (make-cursor :view 1 :x 3 :y 4))))
    (ok (finish session (make-cursor :view 1 :x 4 :y 4)) "moved")))

(deftest cursor-shape-and-visibility-are-changes
  (let ((session (settled-session)))
    (finish session (make-cursor :view 1 :x 0 :y 0))
    (ok (finish session (make-cursor :view 1 :x 0 :y 0 :shape :bar)) "shape")
    (ok (finish session (make-cursor :view 1 :x 0 :y 0 :shape :bar :visible nil))
        "visibility")))

;;; Default colours

(deftest the-first-frame-carries-the-default-colours
  (let* ((session (make-session))
         (frame (finish session)))
    (ok frame "even with nothing else in it")
    (ok (typep (frame-defaults frame) 'defaults))))

(deftest default-colours-are-sent-only-when-they-change
  (let ((session (settled-session)))
    (add-op session (put 1 0 "a"))
    (ok (null (frame-defaults (finish session))) "unchanged")
    (setf (session-defaults session) (make-defaults :background #x1c1c1c))
    (let ((frame (finish session)))
      (ok frame "a colour change alone is worth a frame")
      (ok (eql #x1c1c1c (defaults-background (frame-defaults frame)))))
    (ok (null (finish session)) "and not again")))

(deftest defaults-changed-in-place-are-still-noticed
  (let ((session (settled-session)))
    (setf (defaults-foreground (session-defaults session)) #xdddddd)
    (ok (finish session))))

;;; Modeline memoisation

(defun modeline (view text)
  (make-modeline-painted :view view :puts (list (put view 0 text))))

(deftest an-unchanged-modeline-is-not-resent
  (let ((session (settled-session)))
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
    (let ((session (settled-session)))
      (add-op session (modeline 1 "same"))
      (finish session)
      (add-op session change)
      (ok (add-op session (modeline 1 "same"))
          (format nil "after ~(~A~)" (type-of change))))))

;;; Stacking order

(deftest an-unchanged-stacking-order-is-not-resent
  (let ((session (settled-session)))
    (ok (add-op session (make-views-stacked :views '(1 2 3))))
    (ok (finish session))
    (ok (null (add-op session (make-views-stacked :views '(1 2 3)))))
    (ok (add-op session (make-views-stacked :views '(1 3 2))) "reordered")))

(deftest restacking-does-not-make-a-blank-region-stale
  (let ((session (settled-session)))
    (add-op session (make-rest-cleared :view 1 :y 5))
    (finish session)
    (add-op session (make-views-stacked :views '(1 2)))
    (add-op session (make-rest-cleared :view 1 :y 5))
    (let ((frame (finish session)))
      (ok frame "the new order is sent")
      (ok (= 2 (length (frame-ops frame)))))
    (add-op session (make-rest-cleared :view 1 :y 5))
    (ok (null (finish session)))))

;;; Switching suppression off

(deftest without-suppression-everything-is-sent
  (let ((session (make-session :suppress nil)))
    (finish session)
    (ok (finish session) "an empty frame")
    (add-op session (modeline 1 "same"))
    (ok (add-op session (modeline 1 "same")) "a repeated modeline")
    (add-op session (make-views-stacked :views '(1)))
    (ok (add-op session (make-views-stacked :views '(1))) "a repeated order")
    (ok (zerop (session-dropped-count session)))))
