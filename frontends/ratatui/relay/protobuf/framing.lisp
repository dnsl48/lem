(defpackage :lem-relay/protobuf/framing
  (:use :cl)
  (:export :write-delimited
           :read-delimited))
(in-package :lem-relay/protobuf/framing)

;;; Protobuf's standard length-delimited framing (relay.proto): a varint
;;; byte length, then the message. prost reads it with
;;; decode_length_delimited. Only the framing is written here; the
;;; messages themselves are cl-protobufs' to encode (ADR 0010).

(defun write-varint (value stream)
  (loop :do (let ((byte (ldb (byte 7 0) value)))
              (setf value (ash value -7))
              (if (zerop value)
                  (progn (write-byte byte stream) (return))
                  (write-byte (logior byte #x80) stream)))))

(defun read-varint (stream)
  "The varint at the start of STREAM, or NIL at end of file before its
first byte."
  (loop :with value := 0
        :for shift :from 0 :by 7
        :for byte := (read-byte stream nil nil)
        :do (cond ((null byte)
                   (if (zerop shift)
                       (return nil)
                       (error "The stream ended inside a length prefix.")))
                  ((<= 64 shift)
                   (error "A length prefix longer than a 64-bit varint."))
                  (t
                   (setf value (logior value (ash (ldb (byte 7 0) byte) shift)))
                   (unless (logbitp 7 byte)
                     (return value))))))

(defun write-delimited (stream body)
  "Write BODY, an octet vector holding one message, to the octet STREAM
with its length in front, and flush it."
  (write-varint (length body) stream)
  (write-sequence body stream)
  (force-output stream))

(defun read-delimited (stream)
  "The next message on the octet STREAM, as an octet vector, or NIL when
the stream ends between messages."
  (alexandria:when-let ((length (read-varint stream)))
    (let* ((body (make-array length :element-type '(unsigned-byte 8)))
           (read (read-sequence body stream)))
      (unless (= read length)
        (error "The stream ended inside a message."))
      body)))
