# Этап D — Сеть: полное (TCP-коннекты, DHCP, DNS, HTTP, сокеты) 🔄

**Даты:** начат 2026-08-29
**Статус:** в работе (порция 1: TCP state machine — готова).
**Цель этапа:** рабочие TCP-коннекты, DNS, HTTP(S), сокеты в SDK, DHCP.

## Цель из roadmap (кратко)

1. `dhcp.rs`: DISCOVER→OFFER→REQUEST→ACK, renewal, интеграция в stack.
2. `tcp.rs`: полная state machine (CLOSED/SYN_SENT/ESTABLISHED/
   FIN_WAIT/CLOSE_WAIT/TIME_WAIT…), retransmit/RTO, window,
   сегментация, `TcpListener::accept`; host-тесты «сегмент×состояние».
3. Сокет-ABI v2 (`socket/connect/send/recv/close/bind`) + SDK
   `TcpStream`/`UdpSocket`.
4. `dns.rs`: A/AAAA-резолвер, кэш, /etc/resolv.conf.
5. `http.rs` + TLS (отдельный крейт orbita-tls).
6. Wi-Fi/BT транспорты (для железа, этап J).
7. `netcfg` настройка ip/mask/gw/dns + virtio-net.

## Лог прогресса

### 2026-08-29 — порция 1: TCP state machine (чистая логика) ✅

**Сделано:**
- `orbita-net/src/tcp_state.rs` (новый модуль):
  - `TcpState` — 11 состояний RFC 793 (Closed..TimeWait) +
    `can_send_data`/`is_closed`;
  - `TcpControlBlock` — snd/rcv ISN/NXT-счётчики;
  - `on_segment(seq, ack, flags, len) -> TcpAction` — полный переходный
    матрикс: активное/пассивное открытие (SYN+ACK, simultaneous open),
    данные (in-order ack / out-of-order re-ACK), teardown (FIN→
    CloseWait/FinWait1, simultaneous close→Closing, TimeWait→timeout),
    RST из любого состояния, Drop для мусора;
  - `close()` (FIN жрёт sequence number), `timeout()` (TimeWait expiry),
    `data_sent`/`send_header` для передачи данных;
  - `TcpAction::{Send(SendPlan), Opened, Closed, Drop}` — без I/O, без
    alloc, без таймеров: сокет-слой кормит сегменты и исполняет actions.
- **21 host-тест**: матрица сегмент×состояние (active/passive open,
  simultaneous open/close, out-of-order, дубликаты, retransmitted SYN,
  half-close данные в CloseWait/FinWait2, RST, TIME_WAIT-таймаут).

**Грабли:** первый вариант close() не advancing snd_nxt на FIN —
тесты поймали мгновенно (ACK-валидация FinWait1/Closing ломалась) —
починено, ожидания обновлены.

**Тесты:** orbita-net 46 passed / 0 failed (25→46); workspace 110/0.

**Дальше (порция 2):** интеграция в `stack.rs` — TcpSocket над
TcpControlBlock: parsing→on_segment→build/send через e1000, очередь
retransmit (таймер на poll-тиках), `TcpListener::accept`; loopback-тест
в QEMU (соединение с самим собой).

---
*(шаблон порции: дата → Сделано/Тесты/Дальше; статусы: ⬜ planned,
🔄 in progress, ✅ done, ⚠️ blocked)*

### 2026-08-29 — порция 2: TCP в живой ОС (loopback-коннект) ✅

**Сделано:**
- `orbita-net/src/tcp_socket.rs`: `TcpEndpoint` (адрес-тупл + TCB + rx-буфер
  + parent-listener) + `find_endpoint` (точное совпадение → listener).
- `stack.rs`: TCP-слой — `receive_tcp` (демультиплексирование; SYN на
  listener → спавн child'а со свежим ISN), `tcp_emit` (TCP+IP+Ethernet;
  **software-loopback**: кадр на свой IP → очередь `loopback_rx`, иначе
  pending_tx+ARP), `tcp_pump` (дренаж очереди, до 64 кадров),
  публичный API: `tcp_listen/tcp_connect/tcp_state/tcp_send/tcp_take_rx/
  tcp_accept/tcp_close`.
- **FSM-фиксы, найденные интеграцией**: Listen-ветка не advancing snd_nxt
  на SYN+ACK (child-соединения жили с кривым ISN — данные шли
  out-of-order); SYN+ACK в SYN-SENT теперь валидирует ack == isn+1.
- Ядро: TCP loopback self-test при буте (connect→established, echo
  19 байт, graceful close) — маркеры `tcp loopback connect/echo ok`
  (CI).

**Тесты:** orbita-net 50/0 (+4 loopback: handshake с проверкой
rcv_nxt==snd_nxt обеих сторон, echo roundtrip, close, no-listener);
workspace **114/0**; QEMU ×3: tcp=1/1/1, kill-ok, ring3, boots=1.

**Дальше (порция 3):** SDK-сокеты (`sys::net::TcpStream` через ABI v2),
retransmit/RTO, RST на закрытые порты, сегментация >512 байт, DHCP.
