(in-package :jsonrpc/transport/stdio)

;;;; Fixes for jsonrpc's stdio server transport.
;;;;
;;;; Server-side stdio is not exercised anywhere else in the ecosystem —
;;;; lem-server's other frontends all use websocket — and it carries
;;;; three independent defects. All are fixed here rather than upstream so
;;;; the PoC stays contained; each is worth reporting separately.
;;;;
;;;; 1. Notifications never leave the process. `jsonrpc/server:broadcast'
;;;;    walks `server-client-connections', which is populated by
;;;;    `on-open-connection'. The tcp, websocket and local-domain-socket
;;;;    transports all call it; stdio never does, so the list stays empty
;;;;    and every notification is silently dropped. Request/response still
;;;;    works, because that path uses the connection bound during
;;;;    handling — which is why a `login' round-trip succeeds while no
;;;;    frame ever arrives.
;;;;
;;;; 2. The framing counts characters where the protocol counts bytes.
;;;;    `write-message' sends (length json) as the Content-Length and
;;;;    `read-message' reads into (make-string length), so one non-ASCII
;;;;    character desynchronises the stream for every message after it.
;;;;
;;;; 3. lem-server already tried to fix (2) in
;;;;    frontends/server/jsonrpc-stdio-patch.lisp, but that file is
;;;;    written against an older jsonrpc API: CONNECTION-SOCKET,
;;;;    READ-HEADERS and PARSE-MESSAGE are all interned-but-undefined
;;;;    against the pinned version, so both of its methods signal
;;;;    undefined-function on first use.
;;;;
;;;; These methods replace lem-server's. Ours win because lem-ratatui
;;;; depends on lem-server and therefore loads after it.

(defmethod start-server ((transport stdio-transport))
  "Serve on stdio, registering the connection so notifications can reach it.

Identical to the library's method apart from the `on-open-connection'
call; see defect 1 above."
  (let* ((stream (make-two-way-stream (stdio-transport-input transport)
                                      (stdio-transport-output transport)))
         (connection (make-instance 'connection
                                    :stream stream
                                    :request-callback (transport-message-callback transport))))
    (setf (transport-connection transport) connection)
    (jsonrpc/base:on-open-connection (transport-jsonrpc transport) connection)
    (lem-ratatui/transport:debug-log
     "start-server: registered, connections=~A"
     (length (jsonrpc/server::server-client-connections (transport-jsonrpc transport))))
    (let ((thread
            (bt2:make-thread
             (lambda ()
               (run-processing-loop transport connection))
             :name "jsonrpc/transport/stdio processing")))
      (unwind-protect (run-reading-loop transport connection)
        (bt2:destroy-thread thread)))))

(defmethod send-message-using-transport ((transport stdio-transport) connection message)
  "Write MESSAGE with a byte-counted Content-Length header."
  (let ((json (with-output-to-string (s)
                (yason:encode message s)))
        (stream (jsonrpc/connection:connection-stream connection)))
    (lem-ratatui/transport:debug-log "send ~A bytes" (length json))
    (format stream "Content-Length: ~A~C~C~:*~:*~C~C~A"
            (babel:string-size-in-octets json :encoding :utf-8)
            #\Return
            #\Newline
            json)
    (force-output stream)))

(defmethod receive-message-using-transport ((transport stdio-transport) connection)
  "Read one message, consuming exactly Content-Length bytes.

Characters are read one at a time and the remaining byte count decremented
by each one's encoded width, because the stream is a character stream and
the header counts bytes. Only input events travel this direction, so the
per-character cost is immaterial."
  (let* ((stream (jsonrpc/connection:connection-stream connection))
         (headers (jsonrpc/request-response::read-headers stream))
         (length (ignore-errors (parse-integer (gethash "content-length" headers)))))
    (when length
      (let ((body (with-output-to-string (out)
                    (loop :with remaining := length
                          :while (plusp remaining)
                          :for char := (read-char stream nil nil)
                          :while char
                          :do (write-char char out)
                              (decf remaining
                                    (babel:string-size-in-octets (string char)
                                                                 :encoding :utf-8))))))
        (jsonrpc/request-response:parse-message body)))))
