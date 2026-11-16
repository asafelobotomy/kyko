#ifndef KYKO_GRIDVIEW_H
#define KYKO_GRIDVIEW_H

#include <QtCore/QHash>
#include <QtGui/QFont>
#include <QtGui/QImage>
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
    void keyPressEvent(QKeyEvent* e) override;
    void inputMethodEvent(QInputMethodEvent* e) override;
    QVariant inputMethodQuery(Qt::InputMethodQuery query) const override;
    void focusInEvent(QFocusEvent*) override;

public:
    // The config or theme changed: new font, new glyphs, new cell size.
    void reloadLook();

private:
    void applyFont();
    // Tells Rust how many columns and rows fit, when that changed.
    void fit();

private:
    struct Slot {
        float u0, v0, u1, v1;
        bool wide;
    };
    Slot slotFor(quint32 ch, quint8 style);

    QFont m_font;
    qreal m_cellW = 8, m_cellH = 16, m_ascent = 12;
    int m_cols = 0, m_rows = 0;
    int m_curX = -1, m_curY = -1;
    int m_pane = 0;

    QImage m_atlas;
    QHash<quint64, Slot> m_slots;
    int m_nextSlot = 0;
    bool m_atlasDirty = true;
    QSGTexture* m_texture = nullptr;
};

// Called from Rust, on any thread: asks the item to draw again.
void grid_request_repaint();
// Names the window's application (Wayland app id, the `--class` option).
void app_set_id(rust::Str id);
// Delivers a bus command to the controller, on the GUI thread, from any thread.
void controller_send(int code);
// The layout changed: tells the controller, on the GUI thread, from any thread.
void controller_layout_changed();
// The look changed: asks the item and the controller to reload, on the GUI thread.
void grid_reload_look();
// Registers GridView with QML as org.kyko.app.GridView.
void grid_register();

#endif
