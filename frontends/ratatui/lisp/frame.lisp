(defpackage :lem-ratatui/frame
  (:use :cl)
  (:import-from :lem-ratatui/implementation
   :ratatui)
  (:local-nicknames (:queue :lem/common/queue))
  (:import-from :alexandria :when-let)
  (:export :dropped-count
           :sent-count
           :reset-counters))
(in-package :lem-ratatui/frame)

(defparameter +inert-methods+ '("update-display" "redraw-view-after")
  "Methods that say nothing about what should be painted.

`update-display' marks the end of a frame and `redraw-view-after' carries
only a view reference. A frame consisting solely of these — plus a cursor
that has not moved — changes nothing on screen.")

(defvar *last-cursor* (make-hash-table :test 'eql)
  "View id -> the `move-cursor' argument last sent for it.")

(defvar *cleared-from* (make-hash-table :test 'eql)
  "View id -> the row a `clear-eob' last blanked that view from.

`redraw-lines' emits `clear-eob' on every redraw where the buffer does
not fill the window, so a small file in a large window produces one every
frame. Re-blanking a region that is already blank changes nothing, and
this is what lets those frames be recognised as no-ops. Any other paint
sent to the view invalidates the entry.")

(defvar *dropped* 0)
(defvar *sent* 0)

(defun dropped-count () *dropped*)
(defun sent-count () *sent*)

(defun reset-counters ()
  "Zero the frame counters. Intended for tests."
  (setf *dropped* 0 *sent* 0))

(defun message-method (message) (gethash "method" message))
(defun message-argument (message) (gethash "argument" message))

(defun message-view-id (message)
  "The view a MESSAGE addresses, by either of the two shapes on the wire.

Most messages carry `viewInfo', but `make-view' sends the whole view and
so carries `id' directly."
  (let ((argument (message-argument message)))
    (when (hash-table-p argument)
      (let ((info (gethash "viewInfo" argument)))
        (if (hash-table-p info)
            (gethash "id" info)
            (gethash "id" argument))))))

(defun cursor-view-id (argument)
  (let ((info (and (hash-table-p argument) (gethash "viewInfo" argument))))
    (and (hash-table-p info) (gethash "id" info))))

(defun redundant-clear-eob-p (message)
  "True when this `clear-eob' blanks a region already blanked."
  (let ((id (message-view-id message))
        (y (gethash "y" (message-argument message))))
    (and id y (eql y (gethash id *cleared-from*)))))

(defun paints-p (message)
  "True when MESSAGE changes what the display half shows.

A `move-cursor' repeating the position already sent is not a change. It
is compared rather than treated as always-inert so that a display half
which does render the cursor still sees every real movement."
  (let ((method (message-method message)))
    (cond ((member method +inert-methods+ :test #'equal) nil)
          ((equal method "move-cursor")
           (let* ((argument (message-argument message))
                  (id (cursor-view-id argument)))
             (not (and id (equalp argument (gethash id *last-cursor*))))))
          ((equal method "clear-eob") (not (redundant-clear-eob-p message)))
          (t t))))

(defun remember (messages)
  "Record what this frame leaves on the display half's screen."
  ;; Anything painted into a view makes its cleared region stale. Done
  ;; first so a clear-eob later in the same frame still registers: Lem
  ;; renders lines and *then* blanks what is left over.
  (dolist (message messages)
    (let ((method (message-method message)))
      (unless (or (member method +inert-methods+ :test #'equal)
                  (equal method "move-cursor")
                  (equal method "clear-eob"))
        (alexandria:when-let ((id (message-view-id message)))
          (remhash id *cleared-from*)))))
  (dolist (message messages)
    (let ((method (message-method message)))
      (cond ((equal method "move-cursor")
             (let* ((argument (message-argument message))
                    (id (cursor-view-id argument)))
               (when id (setf (gethash id *last-cursor*) argument))))
            ((equal method "clear-eob")
             (let ((id (message-view-id message))
                   (y (gethash "y" (message-argument message))))
               (when (and id y) (setf (gethash id *cleared-from*) y))))))))

(defmethod lem-server::notify-all ((implementation ratatui))
  "Send the frame only when something in it changes the screen.

Lem redraws after every command, and most of those redraws produce a
frame whose entire content is `update-display', `redraw-view-after' and a
`move-cursor' that repeats the last position — nothing the display half
would paint differently. Sending it costs a serialisation, a wire
round-trip, a decode and a wakeup in both processes to achieve nothing.

`lem-server::notify-all' is internal but generic, so this specialises on
a subclass rather than replacing a method."
  (let ((messages (let ((queue (lem-server::jsonrpc-message-queue implementation)))
                    (loop :until (queue:empty-p queue)
                          :collect (queue:dequeue queue)))))
    (cond ((null messages))
          ((notany #'paints-p messages)
           (incf *dropped*)
           (lem-ratatui/transport:debug-log
            "frame dropped (~A inert messages); ~A dropped / ~A sent"
            (length messages) *dropped* *sent*))
          (t
           (incf *sent*)
           (lem-ratatui/transport:debug-log
            "frame sent (~A messages: ~{~A~^ ~}); ~A dropped / ~A sent"
            (length messages)
            (remove-duplicates (mapcar #'message-method messages) :test #'equal)
            *dropped* *sent*)
           (remember messages)
           (lem-server::notify implementation "bulk" (coerce messages 'vector))))))
