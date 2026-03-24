# Domofon

Неофициальный API клиент для системы **«Умный Дом.ру»** (Эр-Телеком / Дом.ру) с веб-интерфейсом, видеостримингом и детекцией звонков через SIP.

Rust бэкенд + HTML/JS фронтенд. Один бинарник, без runtime-зависимостей.

---

## Отказ от ответственности / Disclaimer

> **RU:** Данный проект создан исключительно в образовательных целях и для личного использования. Проект **не аффилирован** с ПАО «Эр-Телеком Холдинг», ООО «Дом.ру» и их дочерними компаниями. Все товарные знаки принадлежат их правообладателям. Использование данного ПО — на ваш собственный риск. Автор не несёт ответственности за любые последствия использования.
>
> **EN:** This project was created for educational and personal use only. It is **not affiliated** with Er-Telecom Holding or Dom.ru. All trademarks belong to their respective owners. Use at your own risk. The author assumes no liability for any consequences of use.

---

## Возможности

- Авторизация по номеру телефона (SMS код)
- Просмотр камер домофонов (снапшоты)
- Live HLS видеострим с камеры
- Архив видео с таймлайном событий (движение, звонки)
- Открытие двери (SIP / Forpost)
- Детекция входящих звонков через SIP протокол
- Уведомления в реальном времени (SSE + Browser Notifications)
- Webhook для интеграции с Home Assistant и другими системами
- Docker-ready (multi-stage build, ~15MB образ)

---

## Быстрый старт

### Локально

```bash
cargo run --release
# Сервер на http://localhost:3000
```

### Docker

```bash
docker compose up -d
# Или с webhook:
WEBHOOK_URL=http://homeassistant:8123/api/webhook/domofon docker compose up -d
```

При первом запуске откройте `http://localhost:3000` и авторизуйтесь по номеру телефона.

---

## Конфигурация

### Переменные окружения

| Переменная    | Описание                                          | По умолчанию         |
| ------------- | ------------------------------------------------- | -------------------- |
| `WEBHOOK_URL` | URL вебхука для уведомлений о звонках             | _(пусто — отключен)_ |
| `RUST_LOG`    | Уровень логирования (trace/debug/info/warn/error) | `info`               |
| `TZ`          | Часовой пояс                                      | `UTC`                |

### Файлы данных (`./data/`)

| Файл              | Описание                                              |
| ----------------- | ----------------------------------------------------- |
| `tokens.json`     | OAuth2 токены (accessToken, refreshToken, operatorId) |
| `config.json`     | Настройки (интервал опроса, URL вебхука)              |
| `sip-device.json` | SIP учётные данные (login, password, realm)           |

Все файлы создаются автоматически. При запуске в Docker монтируйте `./data` как volume.

### config.json

```json
{
  "callPollingIntervalMs": 10000,
  "callWebhookUrl": "http://192.168.1.100:8123/api/webhook/domofon"
}
```

Webhook URL приоритет: `config.json` → `WEBHOOK_URL` env → отключен.

---

## API Reference

Base URL: `http://localhost:3000`

### Авторизация

| Метод | Endpoint               | Body                       | Описание                        |
| ----- | ---------------------- | -------------------------- | ------------------------------- |
| POST  | `/api/login`           | `{phone}`                  | Запрос SMS кода                 |
| POST  | `/api/select-contract` | `{phone, contract}`        | Выбор договора (если несколько) |
| POST  | `/api/confirm`         | `{phone, code, contract?}` | Подтверждение SMS кода          |
| GET   | `/api/session`         | —                          | Статус авторизации              |
| POST  | `/api/refresh`         | —                          | Обновление токена               |
| POST  | `/api/logout`          | —                          | Выход                           |

### Квартиры и устройства

| Метод | Endpoint                               | Описание            |
| ----- | -------------------------------------- | ------------------- |
| GET   | `/api/places`                          | Список квартир      |
| GET   | `/api/places/{placeId}/accesscontrols` | Домофоны квартиры   |
| GET   | `/api/places/{placeId}/cameras`        | Персональные камеры |

### Управление дверью

| Метод | Endpoint             | Body                                     | Описание        |
| ----- | -------------------- | ---------------------------------------- | --------------- |
| POST  | `/api/open-door`     | `{placeId, device}`                      | Открыть дверь   |
| POST  | `/api/open-entrance` | `{placeId, accessControlId, entranceId}` | Открыть подъезд |

### Медиа

| Метод | Endpoint                             | Параметры                          | Описание                          |
| ----- | ------------------------------------ | ---------------------------------- | --------------------------------- |
| GET   | `/api/snapshot/{placeId}/{deviceId}` | `type=SIP\|BUP`, `w`, `h`          | Снапшот камеры (JPEG)             |
| GET   | `/api/stream/{cameraId}`             | —                                  | HLS стрим (возвращает `{url}`)    |
| GET   | `/api/archive/{cameraId}`            | `ts` (unix sec), `tz` (offset sec) | Архив видео с момента ts          |
| GET   | `/api/events/{cameraId}`             | `dateFrom`, `dateTo`               | События камеры (движение, звонки) |
| POST  | `/api/sip-device`                    | `{placeId, accessControlId}`       | Создать SIP устройство            |

### События и вебхуки

| Метод | Endpoint      | Описание                       |
| ----- | ------------- | ------------------------------ |
| GET   | `/api/events` | SSE поток (event type: `call`) |
| POST  | `/api/call`   | Внешний push события звонка    |

**Формат SSE события:**

```json
{
  "eventType": "incoming_call",
  "date": "2026-03-22T17:59:58.003Z",
  "from": "sip:000@XXXXX.nn.domofon.domru.ru",
  "sipMessage": "INVITE sip:... SIP/2.0"
}
```

### Настройки

| Метод | Endpoint      | Описание           |
| ----- | ------------- | ------------------ |
| GET   | `/api/config` | Текущие настройки  |
| POST  | `/api/config` | Обновить настройки |

---

## SIP — детекция звонков

Приложение регистрируется на SIP-сервере домофона (KAZOO) как SIP-устройство. При звонке в домофон сервер отправляет SIP INVITE → приложение детектирует звонок и:

1. Отправляет SSE event всем подключённым браузерам
2. Показывает Browser Notification
3. Вызывает webhook (если настроен)

| Параметр        | Значение                         |
| --------------- | -------------------------------- |
| Протокол        | SIP/2.0 over UDP                 |
| Локальный порт  | 15060/udp                        |
| Аутентификация  | Digest MD5                       |
| Re-registration | Каждые 25 секунд (NAT keepalive) |
| Ответ на INVITE | 180 Ringing → 486 Busy Here      |

SIP credentials создаются автоматически и сохраняются в `data/sip-device.json`.

---

## Веб-интерфейс

Встроенный веб-интерфейс на `http://localhost:3000`:

- **Live камера** — HLS видеострим, клик по снапшоту для запуска
- **Таймлайн** — полоска с засечками событий (зелёные = движение, красные = звонки)
- **Архив** — клик по таймлайну или событию → воспроизведение архива с этого момента
- **Навигация** — кнопки ◀ ▶ для перехода между событиями, LIVE для возврата
- **Открытие двери** — кнопка с иконкой замка
- **Уведомления** — баннер при входящем звонке + Browser Notification
- **Настройки** — webhook URL через UI
- **Fullscreen** — кнопка для полноэкранного просмотра

---

## Docker

### Dockerfile

Multi-stage build: `rust:1-alpine` → `alpine:3` runtime (~15MB).

### docker-compose.yml

```yaml
services:
  domofon:
    build: .
    container_name: domofon
    restart: unless-stopped
    ports:
      - "3000:3000"
      - "15060:15060/udp"
    volumes:
      - ./data:/app/data
    environment:
      - TZ=Europe/Moscow
      - WEBHOOK_URL=${WEBHOOK_URL:-}
```

---

## Структура проекта

```
├── Cargo.toml
├── Dockerfile
├── docker-compose.yml
├── public/
│   └── index.html          # Веб-интерфейс
├── data/                    # Персистентные данные (git-ignored)
│   ├── tokens.json
│   ├── config.json
│   └── sip-device.json
└── src/
    ├── main.rs              # Entry point, инициализация, фоновые задачи
    ├── client.rs            # HTTP клиент к myhome.proptech.ru
    ├── sip.rs               # SIP клиент (UDP, Digest auth, INVITE detect)
    ├── types.rs             # Все структуры данных
    ├── state.rs             # Shared state, персистенция, конфиг
    ├── error.rs             # Обработка ошибок
    └── api/
        ├── mod.rs           # Сборка роутера
        ├── auth.rs          # Авторизация
        ├── places.rs        # Квартиры и устройства
        ├── door.rs          # Управление дверью
        ├── media.rs         # Видео, снапшоты, архив
        ├── config.rs        # Настройки
        └── sse.rs           # SSE и вебхуки
```

---

## Интеграция с Home Assistant

Настройте webhook URL в конфиге или через env:

```bash
WEBHOOK_URL=http://homeassistant.local:8123/api/webhook/domofon
```

В Home Assistant создайте автоматизацию на webhook trigger `domofon` для обработки входящих звонков.

---

## CI/CD

GitHub Actions автоматически билдит Docker образ:

- **PR** → билд для проверки
- **Push в main** → билд + push в `ghcr.io`
- **Тег `v*`** → семантические теги (`1.0.0`, `1.0`, `sha-xxx`)

```bash
# Pull образ из реестра
docker pull ghcr.io/z81/domofon:main
```

---

## License

MIT License. See [Disclaimer](#отказ-от-ответственности--disclaimer).
