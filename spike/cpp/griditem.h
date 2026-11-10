#ifndef KYKO_SPIKE_GRIDITEM_H
#define KYKO_SPIKE_GRIDITEM_H

#include <QtGui/QFont>
#include <QtQuick/QQuickPaintedItem>

#include "rust/cxx.h"

struct FfiCell;

// Paints one pane's grid snapshot and forwards keyboard and input-method input to the engine.
class GridItem : public QQuickPaintedItem {
    Q_OBJECT
    Q_PROPERTY(qreal cellWidth READ cellWidth CONSTANT)
    Q_PROPERTY(qreal cellHeight READ cellHeight CONSTANT)
    Q_PROPERTY(int cols READ cols NOTIFY gridChanged)
    Q_PROPERTY(int rows READ rows NOTIFY gridChanged)

public:
    explicit GridItem(QQuickItem* parent = nullptr);
    ~GridItem() override;

    void paint(QPainter* painter) override;
    qreal cellWidth() const { return m_cellW; }
    qreal cellHeight() const { return m_cellH; }
    int cols() const { return m_cols; }
    int rows() const { return m_rows; }

Q_SIGNALS:
    void gridChanged();

protected:
    void geometryChange(const QRectF& newGeometry, const QRectF& oldGeometry) override;
    void keyPressEvent(QKeyEvent* event) override;
    void inputMethodEvent(QInputMethodEvent* event) override;
    QVariant inputMethodQuery(Qt::InputMethodQuery query) const override;
    void mousePressEvent(QMouseEvent* event) override;

private:
    QFont m_font;
    QFont m_bold;
    qreal m_cellW = 8;
    qreal m_cellH = 16;
    qreal m_ascent = 12;
    int m_cols = 0;
    int m_rows = 0;
    int m_curX = -1;
    int m_curY = -1;
    bool m_started = false;
    struct Stats;
    Stats* m_stats = nullptr;
};

// Register GridItem as org.kyko.spike.native/GridItem. Call before loading QML.
void grid_register();
// Thread-safe: ask the live GridItem to repaint.
void grid_request_repaint();

#endif
