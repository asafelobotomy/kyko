import QtQuick
import QtQuick.Window
import org.kyko.app

Window {
    id: root
    readonly property int pad: 8
    visible: true
    color: "#282828"
    // Follows the theme: the background the grid draws on.
    Connections {
        target: ctl
        function onLookChanged() { root.color = root.hex(ctl.setting("background")) }
    }
    function hex(v) { return "#" + ("000000" + v.toString(16)).slice(-6) }
    Controller { id: ctl }
    title: grid.title.length > 0 ? "kyko: " + grid.title : "kyko"

    GridView {
        id: grid
        anchors.fill: parent
        anchors.margins: root.pad
        focus: true
        onExitStatusChanged: if (exitStatus >= 0) Qt.quit()
    }

    // The first size: 100 columns by 28 rows.
    Component.onCompleted: {
        color = hex(ctl.setting("background"))
        width = Math.round(grid.cellWidth() * 100 + 2 * pad)
        height = Math.round(grid.cellHeight() * 28 + 2 * pad)
        grid.forceActiveFocus()
    }

    // Test aid: KYKO_TEST_SHOT=path saves the grid to a PNG after a second and quits.
    Timer {
        interval: grid.shotDelay
        running: grid.shot.length > 0
        onTriggered: grid.grabToImage(function (r) { r.saveToFile(grid.shot); Qt.quit() })
    }

    Text {
        anchors.centerIn: parent
        visible: grid.notice.length > 0
        color: "#fb4934"
        text: grid.notice
        font.pixelSize: 16
    }
}
