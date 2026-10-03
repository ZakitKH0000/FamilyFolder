# Microsoft Store — что куда вставить в Partner Center

Пакет: `dist\FamilyFolder-<версия>.msix` (собирает `.\scripts\build-store.ps1`). Картинки — в этой папке (`listing\`).
Partner Center → Apps and games → Family Folder → **Start submission** (Начать отправку). Разделы по порядку:

## 1. Pricing and availability (Цены и доступность)
- **Markets:** все рынки (по умолчанию).
- **Visibility:** Public audience, «Make this product available and discoverable in the Store».
- **Pricing:** Free (бесплатно).

## 2. Properties (Свойства)
- **Category:** Utilities & tools, подкатегория — File managers.
- **Privacy policy URL:** `https://github.com/ZakitKH0000/FamilyFolder/blob/main/PRIVACY.md`
- **Website:** `https://github.com/ZakitKH0000/FamilyFolder`
- **Support contact info:** `https://github.com/ZakitKH0000/FamilyFolder/issues`
- «This product accesses, collects, or transmits personal information» — **Yes** (передаёт файлы и сообщения на ваши устройства).
- Остальные галочки — не трогать.

## 3. Age ratings (Возрастной рейтинг, анкета IARC)
- Тип: **Utility, Productivity, Communication, or Other**.
- Насилие, страшное, азартные игры, покупки внутри программы — **No**.
- «Users can interact or exchange content with other users» — **Yes** (сообщения и файлы между своими устройствами).
- «Shares the user's current physical location» — **No**. «Unrestricted internet access / web browser» — **No**.

## 4. Packages (Пакеты)
Перетащить `dist\FamilyFolder-<версия>.msix`. Device families — оставить **Windows 10/11 Desktop**.

## 5. Store listings (Описание в Store)
Добавить языки **English (United States)** и **Русский**. В каждом: описание, снимки (те же 5 картинок, по порядку
`1-papych.png` … `5-moods.png`), подписи к снимкам, функции, слова для поиска. **Store logos** → квадратный 1:1 —
`store-logo-300.png`.

## 6. Submission options (Параметры отправки)
- **Publishing hold options:** «Publish this submission as soon as it passes certification».
- **Restricted capabilities → runFullTrust** — объяснение (по-английски):

```
Family Folder is a Win32 desktop app (Tauri + WebView2). It needs full trust to watch the family folder the user
chose anywhere on disk and save received files there, to show a panel next to File Explorer windows and a bar at
the top of the screen that reacts to the mouse pointer, to pin the folder in File Explorer and create a desktop
shortcut on request, and to open received links and files with their default apps.
```

- **Notes for certification** (подсказка проверяющим):

```
Family Folder shares files and messages directly between a family's computers (peer-to-peer, encrypted, no account).
Testing on one PC: launch the app, choose "Yes, start here" on the welcome screen and press "Start". The main window,
the bar at the top of the screen (move the pointer to the top centre of the screen or drag a file there), messages
and the guided tour (Settings → Papych → Show) work without a second device.
To test a transfer, install the app on a second PC: on the first one open Settings → "Invite a device", on the second
choose "I have a code" and paste the code. Computers find each other through the public iroh relay network (n0).
```

---

## English (United States)

**Product name:** Family Folder

**Description:**

```
Family Folder is a shared folder for the whole family. Put a photo, a video or a document into it — and a moment later it pops up on your brother's, mom's or grandpa's computer, sent straight from your PC over the internet. No cloud storage to fill up, no accounts, no ads.

Carrying your files is Papych — a little robot folder with a big personality. He follows your cursor with his eyes, swallows the files you drop on him and sends them off as paper planes, jumps up to press "Get" for you, sneezes, gets dizzy and naps in a nightcap while transfers are paused.

WHAT IT DOES
• It's just a folder. Drop files in — your family gets "Get / Decline", and the file comes directly from your computer.
• Any size. A photo, a 50 GB video, a whole game folder. Transfers resume after sleep or a dropped connection, and every byte is verified.
• Works anywhere. Different cities, home Wi-Fi, a phone hotspot — computers find each other on their own.
• The bar at the top of the screen. Drag files to the top edge and a sleek black bar slides down: drop on Papych to send to everyone, or on a family member's mini robot to send only to them.
• Messages. Send any text or link to everyone or to one person, with Copy and Open link buttons.
• Safe both ways. Deleting a file never deletes it on other computers; a changed file arrives as a new version; if two people edit at once, both versions are kept.
• A family of any size, connected with one-time invite codes.
• Auto-accept for photos or all files, with a size limit; programs are always asked about.
• Pause, an optional encrypted cloud route (Yandex Disk, WebDAV, OneDrive or Google Drive folder) and 9 languages.
• A guided tour: Papych jumps out of the window and shows you everything with his pointer stick.

PRIVACY
Files travel directly between your computers over an encrypted connection. No accounts, no servers of its own, no analytics.
```

**Short description:**

```
A shared folder for your family's PCs: drop a file in and it goes straight to your brother's computer — delivered by Papych, a robot folder with a big personality.
```

**What's new in this version:** `First release in the Microsoft Store.`

**Product features** (по одной в поле):

```
Share files of any size directly between your family's computers
Papych — an animated robot folder that follows your cursor and delivers your files
A bar at the top of the screen: drag files up, drop on Papych or on one person
Send any text or link with Copy and Open link buttons
Encrypted connection — no accounts, no ads, no analytics
Works across cities and networks and resumes after interruptions
Auto-accept photos or all files, with a size limit
Optional encrypted cloud route for when computers aren't online together
A guided tour with Papych and his pointer stick
9 languages
```

**Screenshot captions:**

```
1-papych.png  — Meet Papych: he swallows your file and flies it to your brother as a paper plane.
2-shade.png   — Drag files to the top of the screen: drop on Papych for everyone, or on one person's robot.
3-tour.png    — After installation Papych jumps out of the window and shows you around.
4-window.png  — Incoming files, messages to the family and simple settings.
5-moods.png   — Papych has moods: he sneezes, gets dizzy, naps during a pause and looks for the signal.
```

**Search terms** (до 7): `file sharing`, `family`, `send files`, `shared folder`, `peer to peer`, `file transfer`, `sync`

**Copyright and trademark info:** `© 2026 Zakir Khalilov`

**Additional license terms:**

```
Free to use. © 2026 Zakir Khalilov. All rights reserved: copying, modifying or redistributing the app or the Papych character without permission is not allowed. Full terms: https://github.com/ZakitKH0000/FamilyFolder/blob/main/LICENSE
```

**Developed by:** `Zakir Khalilov`

---

## Русский

**Product name:** Family Folder *(название в Store одно на все языки; внутри программы — «Общая папка»)*

**Description:**

```
«Общая папка» — общая папка для всей семьи. Положите в неё фото, видео или документ — и через мгновение он появится на компьютере брата, мамы или дедушки: напрямую с вашего компьютера, через интернет. Без облака, которое переполняется, без учётных записей и рекламы.

Доставляет файлы Папыч — маленький робот-папка с большим характером. Он следит глазами за курсором, глотает брошенные на него файлы и отправляет их бумажными самолётиками, сам подпрыгивает и нажимает «Получить», чихает, кружится и спит в ночном колпаке, пока передача на паузе.

ЧТО УМЕЕТ
• Это просто папка. Положите файлы — у родных появится «Получить / Отклонить», и файл придёт напрямую с вашего компьютера.
• Любой размер. Фото, видео на 50 ГБ, целая папка с игрой. После сна или обрыва связи передача продолжится, каждый байт проверяется.
• Работает где угодно. Разные города, домашний Wi-Fi, раздача с телефона — компьютеры сами находят друг друга.
• Шторка сверху экрана. Потащите файлы к верхнему краю — выедет чёрная шторка: бросите на Папыча — получат все, на мини-робота родственника — только он.
• Сообщения. Любой текст или ссылка — всем или одному, с кнопками «Скопировать» и «Открыть ссылку».
• В обе стороны и бережно. Удаление файла у других ничего не удаляет; изменённый файл приходит новой версией; если двое правили одновременно — сохраняются обе.
• Семья любого размера: компьютеры подключаются по одноразовому коду.
• Автоприём фото или всех файлов с пределом размера; о программах всегда спросит.
• Пауза, облако по желанию с шифрованием (Яндекс Диск, WebDAV, папка OneDrive или Google Диска) и 9 языков.
• Экскурсия: Папыч выпрыгивает из окна и с указкой показывает всё.

КОНФИДЕНЦИАЛЬНОСТЬ
Файлы идут напрямую между вашими компьютерами по зашифрованному соединению. Без учётных записей, своих серверов и статистики.
```

**Short description:**

```
Общая папка для компьютеров семьи: положите файл — и он уйдёт прямо на компьютер брата. Доставляет Папыч — робот-папка с большим характером.
```

**What's new in this version:** `Первый выпуск в Microsoft Store.`

**Product features:**

```
Файлы любого размера — напрямую между компьютерами семьи
Папыч — живой робот-папка: следит за курсором и доставляет файлы
Шторка сверху экрана: потащите файлы вверх и бросьте на Папыча или на одного человека
Любой текст или ссылка — с кнопками «Скопировать» и «Открыть ссылку»
Зашифрованное соединение — без учётных записей, рекламы и статистики
Работает между городами и сетями, продолжает после обрыва связи
Автоприём фото или всех файлов с пределом размера
Облако по желанию — если компьютеры не включены одновременно
Экскурсия с Папычем и его указкой
9 языков
```

**Screenshot captions:**

```
1-papych.png  — Знакомьтесь, Папыч: глотает файл и отправляет его брату бумажным самолётиком.
2-shade.png   — Потащите файлы к верху экрана: бросьте на Папыча — всем, на робота человека — только ему.
3-tour.png    — После установки Папыч выпрыгивает из окна и всё показывает.
4-window.png  — Входящие файлы, сообщения семье и простые настройки.
5-moods.png   — У Папыча есть настроение: чихает, кружится, спит на паузе и ищет связь.
```

**Search terms:** `общая папка`, `передача файлов`, `семья`, `отправить файлы`, `обмен файлами`, `синхронизация`, `p2p`

**Copyright and trademark info:** `© 2026 Закир Халилов`

**Additional license terms:**

```
Бесплатно. © 2026 Закир Халилов, все права защищены: копировать, изменять и распространять программу и персонажа Папыча без разрешения нельзя. Подробно: https://github.com/ZakitKH0000/FamilyFolder/blob/main/LICENSE
```

**Developed by:** `Zakir Khalilov`
