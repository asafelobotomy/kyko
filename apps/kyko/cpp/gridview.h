#ifndef KYKO_GRIDVIEW_H
#define KYKO_GRIDVIEW_H

#include <QtCore/QHash>
#include <QtCore/QList>
#include <QtCore/QPair>
#include <QtCore/QTimer>
#include <QtGui/QFont>
#include <QtGui/QImage>
#include <QtGui/QRawFont>
#include <QtQuick/QQuickItem>

#include "rust/cxx.h"

class QSGTexture;

// What QML needs from Rust besides the grid: the window's settings, bus commands, and a signal
// when the config or theme changed. One instance, made by the drop-down's QML.
class Controller : public QObject {
    Q_OBJECT
public:
    explicit Controller(QObject* parent = nullptr);
    ~Controller() override;
    Q_INVOKABLE int setting(const QString& name) const;
    // The window's visibility, for the bus's Visible property.
    Q_INVOKABLE void setVisible(bool visible) const;
    // The tabs as JSON: {active, focused, title, notice, tabs: [{id, name, active, panes, dividers}]}.
    Q_INVOKABLE QString layout() const;
    // The tab bar's buttons and mouse: by the action's name in [keys], and by tab, pane and divider.
    Q_INVOKABLE void action(const QString& name) const;
    Q_INVOKABLE void focusPane(int pane) const;
    Q_INVOKABLE void focusTab(int tab) const;
    Q_INVOKABLE void closeTab(int tab) const;
    Q_INVOKABLE void closePane(int pane) const;
    Q_INVOKABLE void dismiss(int pane) const;
    Q_INVOKABLE void setRatio(int tab, const QString& path, double ratio) const;
    Q_INVOKABLE void renameTab(int tab, const QString& name) const;
    Q_INVOKABLE QString fixedTitle() const;
    // The answer to "close it anyway?".
    Q_INVOKABLE void confirm(bool yes) const;
    // The Unicode input box: matches for what is typed, and closing it (typing `text` or not).
    Q_INVOKABLE QString unicodeSearch(const QString& query) const;
    Q_INVOKABLE void unicodeDone(const QString& text, bool typed) const;
    // The find bar: 0 look for text, 1 next, 2 previous, 3 close.
    Q_INVOKABLE void search(int op, const QString& text) const;
    // Preferences (spec 2.6): the pages and values as JSON; a change or reset returns "" when it
    // was written, else why not.
    Q_INVOKABLE QString prefsPages() const;
    Q_INVOKABLE QString prefsValues() const;
    Q_INVOKABLE QString prefsSet(const QString& key, const QString& json) const;
    Q_INVOKABLE QString prefsReset(const QString& page) const;
    Q_INVOKABLE QString prefsKey(int key, int mods, const QString& text) const;
    // Test aid: where to save the window's screen (KYKO_TEST_SHOT), and after how many ms.
    Q_INVOKABLE QString shot() const;
    Q_INVOKABLE int shotDelay() const;

Q_SIGNALS:
    // 0 toggle, 1 show, 2 hide: from `kyko toggle`, `show` and `hide` over D-Bus; 3 quit: a
    // windowed kyko has nothing left to show.
    void command(int code);
    void lookChanged();
    // The tabs, panes or their names changed: read layout() again.
    void layoutChanged();
};

// One pane's grid, drawn with scene-graph nodes and a glyph atlas (spec 2.4), and the keyboard
// and input-method input that goes with it. The screen itself lives in Rust (session.rs).
class GridView : public QQuickItem {
    Q_OBJECT
    Q_PROPERTY(int cols READ cols NOTIFY gridChanged)
    Q_PROPERTY(int rows READ rows NOTIFY gridChanged)
    // Which pane of the session this item shows; set once by QML.
    Q_PROPERTY(int pane READ pane WRITE setPane NOTIFY paneChanged)
    Q_PROPERTY(QString shot READ shot CONSTANT)
    Q_PROPERTY(int shotDelay READ shotDelay CONSTANT)

public:
    explicit GridView(QQuickItem* parent = nullptr);
    ~GridView() override;
    int cols() const { return m_cols; }
    int rows() const { return m_rows; }
    int pane() const { return m_pane; }
    void setPane(int pane);
    QString shot() const;
    int shotDelay() const { return qEnvironmentVariableIntValue("KYKO_TEST_MS") > 0 ? qEnvironmentVariableIntValue("KYKO_TEST_MS") : 1500; }
    // The cell size in pixels, for sizing a window to a number of columns and rows.
    Q_INVOKABLE qreal cellWidth() const { return m_cellW; }
    Q_INVOKABLE qreal cellHeight() const { return m_cellH; }

Q_SIGNALS:
    void gridChanged();
    void paneChanged();

protected:
    QSGNode* updatePaintNode(QSGNode* old, UpdatePaintNodeData*) override;
    void geometryChange(const QRectF& newGeometry, const QRectF& oldGeometry) override;
    void focusOutEvent(QFocusEvent*) override;
    void keyPressEvent(QKeyEvent* e) override;
    void keyReleaseEvent(QKeyEvent* e) override;
    void inputMethodEvent(QInputMethodEvent* e) override;
    QVariant inputMethodQuery(Qt::InputMethodQuery query) const override;
    void focusInEvent(QFocusEvent*) override;
    void mousePressEvent(QMouseEvent* e) override;
    void mouseMoveEvent(QMouseEvent* e) override;
    void mouseReleaseEvent(QMouseEvent* e) override;
    void hoverMoveEvent(QHoverEvent* e) override;
    void wheelEvent(QWheelEvent* e) override;

public:
    // The config or theme changed: new font, new glyphs, new cell size.
    void reloadLook();
    // The pane's program rang the bell: 1 flashes the pane, 2 plays the desktop's bell sound.
    // Either marks the window urgent when it is not the active one.
    void bell(int mode);

private:
    void applyFont();
    // Tells Rust how many columns and rows fit, when that changed.
    void fit();
    // Sends a mouse event with its cell to Rust.
    void sendMouse(quint8 kind, quint8 button, const QPointF& pos, Qt::KeyboardModifiers mods, quint8 clicks);

private:
    // A picture in the atlas: where it is, and where it goes relative to its anchor (the pen
    // position on the baseline for a glyph, the cell's top-left corner for a drawn shape).
    struct Tile {
        float u0 = 0, v0 = 0, u1 = 0, v1 = 0;
        float ox = 0, oy = 0, w = 0, h = 0;
        bool color = false;  // a colour glyph (emoji): drawn with its own colours
        bool valid = false;  // false: nothing to draw (a space, or no room)
    };
    // One glyph of a shaped run: which font and glyph, the run's cell it belongs to, and its
    // offset from that cell's pen position (a mark over its base, a ligature's own offset).
    struct Shaped {
        int font;
        quint32 glyph;
        int cell;
        float dx, dy;
    };
    const QList<Shaped>& shape(const QString& text, const QList<int>& cellOf, quint8 style);
    Tile glyphTile(int font, quint32 glyph);
    Tile boxTile(quint32 ch);
    bool allocate(int w, int h, int& x, int& y);
    void resetAtlas();
    int fontIndex(const QRawFont& f);

    QFont m_fonts[4];  // regular, bold, italic, bold italic
    QFont m_font;
    bool m_ligatures = true;
    qreal m_cellW = 8, m_cellH = 16, m_ascent = 12, m_shift = 0;
    qreal m_ulPos = 2, m_ulThick = 1, m_strikePos = 6;
    int m_cursorShape = 0;  // 0 block, 1 beam, 2 underline
    bool m_blink = false, m_blinkOn = true;
    QTimer m_blinkTimer;
    bool m_flash = false;  // the visual bell is showing
    uchar m_bgAlpha = 255;  // window.opacity_pct
    // The scrollbar: shown while the view is above the live screen, and draggable.
    quint32 m_scrollOff = 0, m_scrollHist = 0;
    bool m_scrollDrag = false;
    void scrollToY(qreal y);
    int m_cols = 0, m_rows = 0;
    int m_curX = -1, m_curY = -1;
    int m_pane = 0;
    // For counting clicks: when and where the last press was, and how many in a row.
    qint64 m_lastPress = 0;
    QPoint m_lastCell;
    int m_clicks = 0;
    int m_wheel = 0;
    Qt::MouseButtons m_down;
    // A press that did not move off its cell may be a click on a link.
    bool m_dragged = false;
    // The link under the pointer: (row, start, end) triples, underlined while hovered.
    QList<quint16> m_hover;
    void hoverLink(const QPointF& pos);

    QImage m_atlas;
    int m_shelfX = 0, m_shelfY = 0, m_shelfH = 0;
    bool m_atlasDirty = true, m_atlasReset = false;
    QList<QRawFont> m_rawFonts;
    QHash<quint64, Tile> m_tiles;  // (font << 32 | glyph), and box shapes under font 0xffff
    QHash<QString, QList<Shaped>> m_shaped;
    QSGTexture* m_texture = nullptr;
    // Images: their textures by (id, generation), and the cell size the session was told.
    QHash<QPair<quint32, quint64>, QSGTexture*> m_images;
    int m_sentCellW = 0, m_sentCellH = 0;
    void placeImages(QSGNode* under, QSGNode* over);
};

// Puts text on the clipboard, and reads it for a paste into a pane, on the GUI thread.
void controller_copy(rust::Str text, bool primary);
void controller_paste(quint32 pane);
// Called from Rust, on any thread: the pane's program rang the bell.
void grid_bell(quint32 pane, int mode);
// Called from Rust, on any thread: asks the item to draw again.
void grid_request_repaint();
// Names the window's application (Wayland app id, the `--class` option).
void app_set_id(rust::Str id);
// Windows get an alpha channel, so `window.opacity_pct` can let the desktop show through.
void app_set_translucent();
// Delivers a bus command to the controller, on the GUI thread, from any thread.
void controller_send(int code);
// The layout changed: tells the controller, on the GUI thread, from any thread.
void controller_layout_changed();
// The look changed: asks the item and the controller to reload, on the GUI thread.
void grid_reload_look();
// The configured font's cell size in pixels, (width << 16) | height, before any item exists.
quint32 grid_cell_px();
// Registers GridView with QML as org.kyko.app.GridView.
void grid_register();

#endif
