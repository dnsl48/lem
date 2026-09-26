(defpackage :lem-relay/json/framing
  (:use :cl)
  (:export :write-message
           :read-message))
(in-package :lem-relay/json/framing)

;;; LSP-style framing, `Content-Length: N' then a blank line then N bytes,
;;; as today's display half reads and writes it (lem-protocol's
;;; framing.rs). Both functions work on octet streams: the length counts
;;; bytes, and a character stream is how jsonrpc's stdio transport came to
;;; count characters instead (protocol-notes.md, section 13).

(defun write-message (stream body)
  "Write BODY, an octet vector, to the octet STREAM as one framed message,
and flush it."
  (write-sequence (babel:string-to-octets
                   (format nil "Content-Length: ~D~C~C~C~C"
                           (length body) #\Return #\Newline #\Return #\Newline)
                   :encoding :ascii)
                  stream)
  (write-sequence body stream)
  (force-output stream))

(defun read-header-line (stream)
  "The next header line from the octet STREAM without its line ending, or
NIL at end of file before any byte of it."
  (let ((line (make-array 0 :element-type 'character :adjustable t :fill-pointer 0)))
    (loop :for byte := (read-byte stream nil nil)
          :do (cond ((null byte)
                     (return (if (zerop (length line)) nil line)))
                    ((= byte 10)
                     (return (string-right-trim '(#\Return) line)))
                    (t (vector-push-extend (code-char byte) line))))))

(defun read-message (stream)
  "The body of the next framed message on the octet STREAM, as an octet
vector, or NIL when the stream ends between messages."
  (let ((length nil)
        (saw-header nil))
    (loop :for line := (read-header-line stream)
          :do (cond ((null line)
                     (if saw-header
                         (error "The stream ended inside a message header.")
                         (return-from read-message nil)))
                    ((string= line "")
                     (return))
                    (t
                     (setf saw-header t)
                     (let ((prefix "Content-Length:"))
                       (when (and (<= (length prefix) (length line))
                                  (string-equal prefix line :end2 (length prefix)))
                         (setf length (parse-integer line :start (length prefix)
                                                          :junk-allowed t)))))))
    (unless length
      (error "A framed message without a Content-Length header."))
    (let* ((body (make-array length :element-type '(unsigned-byte 8)))
           (read (read-sequence body stream)))
      (unless (= read length)
        (error "The stream ended inside a message body."))
      body)))
