# Requirements Document

## Introduction

Редизайн фронтенда приложения Dither Yuki 2 на основе нового Figma-дизайна. Ключевое изменение архитектуры: переход от модели "множество фильтров на слой" к модели **"один слой = один эффект"**. Каждый слой в панели Layers имеет ровно один назначенный тип эффекта (Dithering, Glitching, Curves, RGB channels). Боковая панель реорганизуется: настройки эффекта выбранного слоя сверху, панель слоёв снизу. Color Lab выносится в отдельное модальное окно, доступное через меню.

## Glossary

- **App**: Корневой React-компонент приложения (Dither Yuki 2)
- **Preview_Window**: Панель предпросмотра изображения с title bar "Preview" и zoom-контролами в footer
- **Effect_Settings_Panel**: Верхняя секция правой боковой панели, отображающая настройки эффекта выбранного слоя
- **Layers_Panel**: Нижняя секция правой боковой панели, содержащая список слоёв и элементы управления (blend mode, opacity)
- **Effect_Chooser_Dialog**: Модальный диалог выбора типа эффекта при создании нового слоя
- **Color_Lab_Window**: Отдельное модальное окно для управления цветовыми палитрами, доступное через меню "Color Lab"
- **Layer**: Единица обработки изображения; в новой модели каждый Layer содержит ровно один эффект
- **Effect_Type**: Тип обработки, назначенный слою: Dithering, Glitching, Curves или RGB channels
- **Image_Source_Layer**: Специальный базовый слой с исходным изображением, не имеющий назначенного эффекта
- **Menu_Bar**: Горизонтальная панель меню в верхней части окна с пунктами: File, Edit, Presets, Color Lab, Help
- **Sidebar**: Правая боковая панель шириной 332px, содержащая Effect_Settings_Panel и Layers_Panel

## Requirements

### Requirement 1: Главный Layout приложения

**User Story:** As a пользователь, I want видеть интерфейс приложения, организованный по новому Figma-дизайну (header 27px + body 741px, preview слева 692px, sidebar справа 332px), so that я могу эффективно работать с инструментами обработки изображений.

#### Acceptance Criteria

1. THE App SHALL отображать layout из трёх зон: Menu_Bar (фиксированная высота 27px, ширина 100% окна), Preview_Window (левая часть body, занимающая всё горизонтальное пространство за вычетом Sidebar) и Sidebar (правая часть body, фиксированная ширина 332px)
2. THE App SHALL поддерживать минимальный размер окна 800×600 пикселей, при котором все три зоны остаются видимыми
3. THE Menu_Bar SHALL содержать пункты меню в порядке слева направо: File, Edit, Presets, Color Lab, Help
4. THE Sidebar SHALL состоять из двух секций: Effect_Settings_Panel (верхняя) и Layers_Panel (нижняя), разделяющих доступное вертикальное пространство в соотношении приблизительно 1:1 (каждая секция занимает не менее 40% и не более 60% высоты Sidebar)
5. THE Preview_Window SHALL содержать title bar высотой 20px с текстом "Preview", отцентрированным по горизонтали, с декоративными горизонтальными линиями по обе стороны от текста, и footer с zoom-контролами (кнопка увеличения "+", текущий процент масштаба, кнопка уменьшения "−")
6. THE Preview_Window footer SHALL отображать значение zoom в диапазоне от 10% до 800% с шагом, определяемым кнопками "+" и "−" (каждое нажатие изменяет масштаб на один пресет из ряда: 10%, 25%, 50%, 100%, 200%, 400%, 800%)

### Requirement 2: Модель "один слой = один эффект"

**User Story:** As a пользователь, I want чтобы каждый слой имел ровно один назначенный эффект, so that структура проекта остаётся простой и предсказуемой — один слой в панели = один визуальный эффект на изображении.

#### Acceptance Criteria

1. THE Layers_Panel SHALL отображать каждый Layer с иконкой, соответствующей его Effect_Type (одна из: Dithering, Glitching, Curves, RGB channels)
2. WHEN пользователь создаёт новый слой, THE App SHALL показать Effect_Chooser_Dialog для выбора типа эффекта перед добавлением слоя
3. WHEN пользователь подтверждает выбор в Effect_Chooser_Dialog, THE App SHALL создать новый Layer с единственным эффектом выбранного Effect_Type и вставить его непосредственно над текущим выбранным слоем (или на вершину стека, если ни один слой не выбран)
4. THE App SHALL не предоставлять пользователю элементов управления для добавления второго эффекта на Layer или удаления единственного эффекта из Layer (эффект является неотъемлемой частью слоя)
5. THE App SHALL не предоставлять пользователю элементов управления для изменения Effect_Type существующего Layer после его создания
6. THE Image_Source_Layer SHALL отображаться в Layers_Panel с отдельной иконкой (image icon), без возможности назначения эффекта и без возможности удаления

### Requirement 3: Панель настроек эффекта (Effect Settings Panel)

**User Story:** As a пользователь, I want видеть и изменять настройки эффекта текущего выбранного слоя в верхней секции Sidebar, so that я могу тонко управлять параметрами обработки.

#### Acceptance Criteria

1. WHEN пользователь выбирает Layer в Layers_Panel, THE Effect_Settings_Panel SHALL отобразить настройки эффекта этого слоя в течение 100 мс после выбора
2. WHEN выбран Layer с эффектом Dithering, THE Effect_Settings_Panel SHALL показать: секцию цветовой палитры (свотчи + dropdown), dropdown "algorithm type" (Bayer 2×2, Bayer 4×4, Bayer 8×8, Custom PNG, Floyd-Steinberg, Atkinson), слайдер "pixel size" (целое число, диапазон 1–32, по умолчанию 1), слайдер "threshold scale" (диапазон 0.1–4.0, шаг 0.1, по умолчанию 1.0), слайдер "levels" (целое число, диапазон 2–256, по умолчанию 4)
3. WHEN пользователь изменяет параметр в Effect_Settings_Panel, THE App SHALL применить изменение к эффекту выбранного слоя и обновить Preview_Window в течение 500 мс после завершения рендеринга
4. WHILE ни один Layer не выбран, THE Effect_Settings_Panel SHALL отображать только заголовок панели без элементов управления
5. WHEN выбран Image_Source_Layer, THE Effect_Settings_Panel SHALL отображать только заголовок панели без элементов управления (настройки недоступны)
6. IF пользователь вводит значение параметра вне допустимого диапазона, THEN THE Effect_Settings_Panel SHALL отклонить ввод и сохранить последнее допустимое значение параметра
7. WHEN пользователь изменяет параметр слайдера перетаскиванием, THE App SHALL применять debounce 100 мс перед отправкой обновлённого значения в Engine

### Requirement 4: Панель слоёв (Layers Panel)

**User Story:** As a пользователь, I want управлять слоями в нижней секции Sidebar (добавлять, удалять, переключать видимость, менять порядок), so that я могу строить композицию из нескольких эффектов.

#### Acceptance Criteria

1. THE Layers_Panel SHALL отображать заголовок "Layers" и элементы управления выбранного слоя: dropdown "blend mode" (варианты: Normal, Multiply, Screen, Overlay, Darken, Lighten, ColorDodge, ColorBurn, HardLight, SoftLight, Difference, Exclusion; default "Normal") и dropdown "opacity" (диапазон 0%–100% с шагом 1%, default "100%")
2. THE Layers_Panel SHALL отображать список слоёв, каждый элемент содержит: иконку видимости (eye), номер/имя слоя, иконку типа эффекта; выбранный Layer SHALL отображаться с визуальным выделением (highlight)
3. THE Layers_Panel SHALL отображать Image_Source_Layer внизу списка с иконкой изображения; Image_Source_Layer SHALL всегда оставаться в нижней позиции списка
4. WHEN пользователь нажимает кнопку "add layer" (иконку plus в footer панели), THE App SHALL открыть Effect_Chooser_Dialog
5. WHEN пользователь нажимает кнопку trash в footer Layers_Panel и выбран Layer (не Image_Source_Layer), THE App SHALL удалить выбранный Layer и обновить Preview_Window
6. IF при нажатии кнопки trash выбран Image_Source_Layer или ни один Layer не выбран, THEN THE App SHALL оставить кнопку trash неактивной (disabled)
7. WHEN пользователь нажимает иконку видимости (eye) на слое, THE App SHALL переключить видимость этого Layer и обновить Preview_Window
8. THE Layers_Panel SHALL поддерживать drag-and-drop для изменения порядка слоёв, за исключением Image_Source_Layer, который не может быть перемещён
9. WHEN пользователь изменяет значение "blend mode" или "opacity" в Layers_Panel, THE App SHALL применить новое значение к выбранному Layer и обновить Preview_Window

### Requirement 5: Диалог выбора эффекта (Effect Chooser Dialog)

**User Story:** As a пользователь, I want выбирать тип эффекта из визуального каталога при создании нового слоя, so that я понимаю какие эффекты доступны и что делает каждый из них.

#### Acceptance Criteria

1. THE Effect_Chooser_Dialog SHALL отображаться как модальное окно (364x468px) с заголовком "Effect" поверх полупрозрачного overlay, блокирующего взаимодействие с остальным интерфейсом
2. THE Effect_Chooser_Dialog SHALL показывать список из четырёх типов эффектов в фиксированном порядке: Dithering, Glitching, Curves, RGB channels — каждый элемент содержит иконку типа эффекта и текстовое название
3. WHEN пользователь выбирает тип эффекта в Effect_Chooser_Dialog, THE App SHALL создать новый Layer с этим эффектом, вставить его над текущим выбранным слоем в Layers_Panel, сделать новый слой выбранным и закрыть диалог
4. WHEN пользователь закрывает Effect_Chooser_Dialog без выбора (клавиша Escape, click по overlay вне диалога или кнопка закрытия диалога), THE App SHALL закрыть диалог без создания слоя, сохранив текущее состояние документа без изменений
5. WHILE Effect_Chooser_Dialog открыт, THE App SHALL поддерживать навигацию по списку эффектов с клавиатуры (стрелки вверх/вниз для перемещения фокуса, Enter для подтверждения выбора)

### Requirement 6: Color Lab как отдельное окно

**User Story:** As a пользователь, I want открывать Color Lab как отдельное модальное окно через меню, so that рабочее пространство остаётся чистым, а палитры редактируются в выделенном интерфейсе.

#### Acceptance Criteria

1. WHEN пользователь выбирает пункт "Color Lab" в Menu_Bar, THE App SHALL открыть Color_Lab_Window как модальное окно (692x648px), блокирующее взаимодействие с основным интерфейсом до закрытия
2. THE Color_Lab_Window SHALL содержать секцию "Auto extract": dropdown алгоритма (MedianCut, KMeans), слайдер количества цветов (диапазон 2–256, шаг 1, default 8), кнопки "Extract from row frame" и "Extract from actual frame"
3. THE Color_Lab_Window SHALL содержать кнопки импорта/экспорта палитр с поддержкой форматов: ASE, GPL, HEX/TXT, JSON
4. THE Color_Lab_Window SHALL содержать секцию ручного редактирования: список цветов с hex-инпутами (формат #RRGGBB, максимум 256 записей), кнопка удаления для каждого цвета, кнопка "add color +"
5. THE Color_Lab_Window SHALL отображать полосу предпросмотра, показывающую первые 6 цветов палитры как свотчи равной ширины, и кнопки "Sort by brightness" (пересортировать цвета по Oklab lightness по возрастанию) и "Auto interpolate" (заполнить палитру промежуточными цветами между существующими до количества, заданного слайдером)
6. THE Color_Lab_Window SHALL содержать кнопки "Cancel" (закрыть без сохранения) и "Apply" (сохранить палитру в Document и закрыть окно)
7. WHEN пользователь нажимает "Apply", THE Color_Lab_Window SHALL сохранить текущую палитру как Palette в Document (создать новую или обновить существующую, если палитра была загружена для редактирования) и закрыть окно
8. WHEN пользователь нажимает "Cancel" или Escape, THE Color_Lab_Window SHALL закрыть окно, отбросив все несохранённые изменения палитры без модификации Document
9. IF пользователь вводит невалидное hex-значение (не соответствует формату #RRGGBB), THEN THE Color_Lab_Window SHALL отобразить визуальную индикацию ошибки на соответствующем инпуте и не применять невалидное значение к палитре
10. IF пользователь нажимает "Extract from row frame" или "Extract from actual frame" при отсутствии загруженного изображения в документе, THEN THE Color_Lab_Window SHALL отобразить сообщение об ошибке, указывающее на отсутствие изображения для извлечения
11. IF пользователь нажимает "add color +" при количестве цветов равном 256, THEN THE Color_Lab_Window SHALL не добавлять новый цвет и отобразить индикацию достижения максимума

### Requirement 7: Миграция с Filter List на Effect-per-Layer

**User Story:** As a разработчик, I want чтобы фронтенд использовал модель "один эффект на слой" вместо текущего filter-list с множественными фильтрами, so that UI-логика соответствует новому дизайну и API backend упрощается.

#### Acceptance Criteria

1. THE App SHALL удалить компоненты FilterList и FilterPanel из текущей структуры Sidebar и заменить их на Effect_Settings_Panel, который отображает параметры единственного эффекта выбранного слоя
2. WHEN Layer создаётся через Effect_Chooser_Dialog, THE App SHALL вызвать IPC-команду add_layer с параметром effect_type и начальными параметрами эффекта (identity-кривая для Curves, input_black=0/input_white=1/gamma=1 для Levels, algorithm=FloydSteinberg/color_depth=4 для Dither, intensity=0.5/type=RGBShift для Glitch)
3. WHEN пользователь изменяет настройки эффекта в Effect_Settings_Panel, THE App SHALL вызвать IPC-команду update_filter с layer_id выбранного слоя и filter_id единственного FilterInstance этого слоя
4. WHEN App загружает документ, THE App SHALL валидировать, что каждый Layer (кроме Image_Source_Layer) имеет ровно один FilterInstance
5. IF при загрузке документа Layer (кроме Image_Source_Layer) содержит ноль или более одного FilterInstance, THEN THE App SHALL отобразить сообщение об ошибке, указывающее на некорректную структуру документа, и не загружать документ
6. IF IPC-команда add_layer или update_filter возвращает ошибку, THEN THE App SHALL отобразить уведомление об ошибке и сохранить предыдущее состояние UI без применения изменений

### Requirement 8: Preview Window с retro-стилизацией

**User Story:** As a пользователь, I want чтобы preview-окно имело retro Mac OS стилизацию с title bar и zoom-контролами в footer, so that визуальный стиль приложения соответствует задуманной эстетике.

#### Acceptance Criteria

1. THE Preview_Window SHALL отображать title bar в стиле retro Mac OS с текстом "Preview" по центру и горизонтальными линиями-декорациями по обе стороны от текста
2. THE Preview_Window SHALL отображать footer с тремя элементами в ряд: кнопка minus (zoom out), текст текущего масштаба в процентах (целое число от 1 до 6400), кнопка plus (zoom in)
3. WHEN пользователь нажимает plus в footer Preview_Window, THE App SHALL увеличить масштаб к следующему значению в последовательности пресетов [25%, 50%, 100%, 200%, 400%], или на множитель 2× если текущий масштаб выше 400%
4. WHEN пользователь нажимает minus в footer Preview_Window, THE App SHALL уменьшить масштаб к предыдущему значению в последовательности пресетов [25%, 50%, 100%, 200%, 400%], или на множитель 0.5× если текущий масштаб ниже 25%
5. IF текущий масштаб равен минимальному (1%), THEN THE App SHALL отображать кнопку minus в неактивном (disabled) состоянии
6. IF текущий масштаб равен максимальному (6400%), THEN THE App SHALL отображать кнопку plus в неактивном (disabled) состоянии
7. THE Preview_Window SHALL занимать всю доступную высоту body (высота окна минус высота Menu_Bar 27px) и всю ширину окна за вычетом Sidebar (332px)

### Requirement 9: Menu Bar по Figma-дизайну

**User Story:** As a пользователь, I want видеть menu bar с пятью пунктами (File, Edit, Presets, Color Lab, Help), so that я могу быстро найти нужные функции приложения через привычную навигацию.

#### Acceptance Criteria

1. THE Menu_Bar SHALL отображать пять пунктов слева направо в следующем порядке: File, Edit, Presets, Color Lab, Help
2. WHEN пользователь нажимает "File" в Menu_Bar, THE App SHALL показать dropdown с пунктами: Open Image, Save/Export
3. WHEN пользователь нажимает "Color Lab" в Menu_Bar, THE App SHALL открыть Color_Lab_Window (без промежуточного dropdown)
4. WHILE dropdown любого пункта меню открыт, THE Menu_Bar SHALL переключать dropdown на другой пункт при наведении курсора на него
5. WHEN пользователь нажимает за пределами открытого dropdown или нажимает Escape, THE App SHALL закрыть dropdown
6. WHILE курсор наведён на пункт Menu_Bar, THE Menu_Bar SHALL отображать этот пункт с чёрным фоном и белым текстом
7. THE Menu_Bar SHALL иметь высоту 27px, шрифт ChicagoFLF размером 12px и фон цвета #D9D9D9
8. WHEN пользователь нажимает "Edit" в Menu_Bar, THE App SHALL показать dropdown с пунктами: Undo, Redo (пункты отображаются как disabled, если соответствующее действие недоступно)
