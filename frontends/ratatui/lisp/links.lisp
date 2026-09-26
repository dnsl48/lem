(defpackage :lem-ratatui/links
  (:use :cl)
  (:export :*links*
           :find-urls
           :scan-links
           :link-overlays))
(in-package :lem-ratatui/links)

(defvar *links* t
  "Whether web addresses in buffers become terminal hyperlinks (ADR 0018).
Set to NIL in init.lisp to draw them as plain text.")

;;; What reaches a frontend is drawing objects, which carry an attribute
;;; but no buffer position, so a URL is carried in an attribute: an overlay
;;; over each web address, whose attribute sets only `lem:attribute-link'.
;;; Lem merges overlays over the text's own attributes when drawing, so
;;; the text looks as it always did, in every theme, and the relay reads
;;; the URL off the merged attribute (`lem-relay/frame:relayed-link').
;;;
;;; The addresses are found here, not taken from Lem's `link-mode': that is
;;; on only in some buffers (Lisp, shell, the first one), and Markdown puts
;;; its own links only when tree-sitter is not there to highlight it.

(defparameter *url-scanner*
  (ppcre:create-scanner "https?://[^\\s<>\"'`]+")
  "A web address, up to whitespace, quotes or angle brackets; trailing
punctuation is trimmed off by `trim-url'.")

(defparameter *trailing-punctuation* ".,;:!?"
  "Characters that end a sentence around an address rather than the
address itself.")

(defparameter *brackets* '((#\) . #\() (#\] . #\[) (#\} . #\{))
  "Closing brackets, and their openers: one closing an address's own
opener is part of it, as in Wikipedia's, and any other is not.")

(defun trim-url (url)
  "URL without the punctuation and unmatched closing brackets that
surround an address in prose or Markdown: \"(see https://x.org).\" links
https://x.org."
  (loop :for end := (length url)
        :for last := (char url (1- end))
        :for opener := (cdr (assoc last *brackets*))
        :while (or (find last *trailing-punctuation*)
                   (and opener
                        (> (count last url) (count opener url))))
        :do (setf url (subseq url 0 (1- end)))
        :finally (return url)))

(defun find-urls (string)
  "The web addresses in STRING, as a list of (START END URL)."
  (let ((found '()))
    (ppcre:do-matches (start end *url-scanner* string)
      (let ((url (trim-url (subseq string start end))))
        (when (> (length url) (length "https://"))
          (push (list start (+ start (length url)) url) found))))
    (nreverse found)))

(defun link-overlay-p (overlay)
  (lem:overlay-get overlay 'link))

(defun link-overlays (buffer)
  "The link overlays this frontend keeps in BUFFER."
  (remove-if-not #'link-overlay-p (lem:buffer-overlays buffer)))

(defun make-link-overlay (start end url)
  (let ((overlay (lem:make-overlay start end (lem:make-attribute :link url))))
    (lem:overlay-put overlay 'link t)
    overlay))

(defun clear-link-overlays (start end)
  "Delete the link overlays touching START to END."
  (dolist (overlay (link-overlays (lem:point-buffer start)))
    (when (and (lem:point<= (lem:overlay-start overlay) end)
               (lem:point<= start (lem:overlay-end overlay)))
      (lem:delete-overlay overlay))))

(defun scan-links (start end)
  "Put a link overlay over every web address on the lines from START to
END, in place of the ones there before.

On `lem:after-syntax-scan-hook', globally: Lem runs it for every buffer,
after each change and when a buffer is first highlighted."
  (when *links*
    (lem:with-point ((line start)
                     (end end)
                     (from start)
                     (to start))
      (lem:line-start line)
      (lem:line-end end)
      (clear-link-overlays line end)
      (loop :do (loop :for (s e url) :in (find-urls (lem:line-string line))
                      :do (lem:move-point from line)
                          (lem:character-offset from s)
                          (lem:move-point to line)
                          (lem:character-offset to e)
                          (make-link-overlay from to url))
            :until (lem:same-line-p line end)
            :while (lem:line-offset line 1)))))

(lem:add-hook (lem:variable-value 'lem:after-syntax-scan-hook :global) 'scan-links)
