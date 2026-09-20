(defpackage :lem-ratatui/modeline
  (:use :cl)
  (:import-from :lem-ratatui/implementation
   :ratatui)
  (:export :suppressed-count
           :emitted-count
           :reset-counters))
(in-package :lem-ratatui/modeline)

(defvar *capturing* nil
  "True while modeline notifications are being collected rather than sent.")

(defvar *captured* '()
  "Notifications collected during a capture, in reverse order.")

(defvar *last-modeline* (make-hash-table :test 'eql)
  "View id -> the notification list last sent for that view's modeline.")

(defvar *suppressed* 0)
(defvar *emitted* 0)

(defun suppressed-count () *suppressed*)
(defun emitted-count () *emitted*)

(defun reset-counters ()
  "Zero the suppression counters. Intended for tests."
  (setf *suppressed* 0 *emitted* 0))

(defun forget (view)
  "Drop VIEW's cached modeline so the next render is sent unconditionally."
  (remhash (lem-server/view:view-id view) *last-modeline*))

(defmethod lem-server::notify* ((implementation ratatui) method argument)
  "Collect the notification instead of queueing it while capturing.

`lem-server::notify*' is internal, but it is a generic function, so this
is ordinary specialisation on a subclass rather than a patch: no method
is being replaced."
  (if *capturing*
      (push (cons method argument) *captured*)
      (call-next-method)))

(defmethod lem-if:render-line-on-modeline ((implementation ratatui)
                                           view left-objects right-objects
                                           default-attribute height)
  "Send the modeline only when it differs from the last one sent.

`lem-server' repaints the modeline in full on every frame — a
full-width blank `modeline-put' followed by every object, with no
caching — which is most of the traffic in a frame where nothing of
substance changed. Lem line-diffs buffer content via its fingerprint
cache, but the modeline is exempt from that.

Rather than duplicate the rendering, the inherited method runs with its
output diverted into a list, which is compared against the last one sent
for this view. Identical output is dropped.

This is safe only because the display half never clears a view's modeline
except when the view is created or resized, and both invalidate the cache
below. A display half that cleared it independently would go stale."
  (let ((captured (let ((*capturing* t)
                        (*captured* '()))
                    (call-next-method)
                    (nreverse *captured*)))
        (id (lem-server/view:view-id view)))
    (cond ((equalp captured (gethash id *last-modeline*))
           (incf *suppressed*))
          (t
           (setf (gethash id *last-modeline*) captured)
           (incf *emitted*)
           (loop :for (method . argument) :in captured
                 :do (lem-server::notify* implementation method argument))))))

(defmethod lem-if:make-view :around ((implementation ratatui)
                                     window x y width height use-modeline)
  "Forget the cached modeline: a new view starts with a blank one."
  (let ((view (call-next-method)))
    (forget view)
    view))

(defmethod lem-if:set-view-size :before ((implementation ratatui) view width height)
  "Forget the cached modeline: resizing reallocates it blank."
  (forget view))

(defmethod lem-if:delete-view :before ((implementation ratatui) view)
  (forget view))
