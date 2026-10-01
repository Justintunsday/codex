import XCTest

final class WorkspaceUITests: XCTestCase {
    func testNativeWorkspaceScreens() {
        let app = XCUIApplication()
        app.launch()
        XCTAssertTrue(app.descendants(matching: .any).matching(identifier: "promptField").firstMatch.waitForExistence(timeout: 15))
        attach("Conversation", app: app)
        let connection = app.buttons["modelConnection"]
        XCTAssertTrue(connection.waitForExistence(timeout: 5))
        connection.tap()
        XCTAssertTrue(app.navigationBars["Settings"].waitForExistence(timeout: 5))
        openPage("Task", app: app)
        XCTAssertTrue(app.descendants(matching: .any).matching(identifier: "promptField").firstMatch.waitForExistence(timeout: 5))
        for page in ["Files", "Activity", "Settings"] {
            openPage(page, app: app)
            attach(page, app: app)
        }
        XCUIDevice.shared.orientation = .landscapeLeft
        attach("Landscape settings", app: app)
        XCUIDevice.shared.orientation = .portrait
    }

    func testDarkAppearanceWithLargeDynamicType() {
        let app = XCUIApplication()
        app.launchArguments = ["-AppleInterfaceStyle", "Dark", "-UIPreferredContentSizeCategoryName", "UICTContentSizeCategoryAccessibilityXXXL"]
        app.launchEnvironment["CODEX_UI_TEST_APPEARANCE"] = "dark"
        app.launch()
        XCTAssertTrue(app.descendants(matching: .any).matching(identifier: "promptField").firstMatch.waitForExistence(timeout: 15))
        XCTAssertTrue(app.tabBars.buttons["Task"].exists, "Accessibility sizes use the full-width workspace on both device families")
        attach("Dark conversation with large text", app: app)
    }

    func testNativeTerminalRendersANSIAndUnicode() {
        let app = XCUIApplication()
        app.launchArguments = ["-UIPreferredContentSizeCategoryName", "UICTContentSizeCategoryAccessibilityXXXL"]
        app.launchEnvironment["CODEX_UI_TEST_TERMINAL"] = "1"
        app.launch()
        XCTAssertTrue(app.descendants(matching: .any).matching(identifier: "promptField").firstMatch.waitForExistence(timeout: 15))
        openPage("Activity", app: app)
        let terminal = app.segmentedControls["activitySelection"].buttons["Terminal"]
        XCTAssertTrue(terminal.waitForExistence(timeout: 5))
        terminal.tap()
        XCTAssertTrue(app.descendants(matching: .any).matching(identifier: "nativeTerminal").firstMatch.waitForExistence(timeout: 5))
        attach("Native ANSI and Unicode terminal", app: app)
    }

    func testReviewedEditSurvivesRestartAndInterruptedReviewDoesNotWrite() {
        let app = XCUIApplication()
        app.launchEnvironment["CODEX_UI_TEST_PROJECT"] = "1"
        app.launchEnvironment["CODEX_UI_TEST_RESET"] = "1"
        app.launch()
        openFixtureFile(app)
        XCTAssertEqual(app.staticTexts["fileContents"].label, "Hello from the project.\n")
        app.buttons["Edit"].tap()
        let editor = app.textViews["fileEditor"]
        XCTAssertTrue(editor.waitForExistence(timeout: 5))
        editor.tap()
        editor.typeText("Reviewed edit.\n")
        app.buttons["reviewFileChange"].tap()
        XCTAssertTrue(app.buttons["approveChange"].waitForExistence(timeout: 5))
        let start = app.staticTexts["reviewDiffStart"]
        XCTAssertTrue(start.waitForExistence(timeout: 5))
        XCTAssertLessThan(start.frame.minY, app.segmentedControls.firstMatch.frame.maxY + 60)
        attach("File diff awaiting approval", app: app)
        app.buttons["approveChange"].tap()
        let file = app.staticTexts["fileContents"]
        let saved = NSPredicate(format: "label CONTAINS %@", "Reviewed edit.")
        expectation(for: saved, evaluatedWith: file)
        waitForExpectations(timeout: 5)
        attach("Saved file", app: app)

        app.terminate()
        app.launchEnvironment.removeValue(forKey: "CODEX_UI_TEST_RESET")
        app.launch()
        openFixtureFile(app)
        XCTAssertTrue(app.staticTexts["fileContents"].label.contains("Reviewed edit."))
        let baseline = app.staticTexts["fileContents"].label
        app.buttons["Edit"].tap()
        app.textViews["fileEditor"].tap()
        app.textViews["fileEditor"].typeText("Unapproved edit.\n")
        app.buttons["reviewFileChange"].tap()
        XCTAssertTrue(app.buttons["approveChange"].waitForExistence(timeout: 5))
        app.terminate()
        app.launch()
        openFixtureFile(app)
        XCTAssertEqual(app.staticTexts["fileContents"].label, baseline)
        attach("Restart retains approved file", app: app)
    }

    private func openFixtureFile(_ app: XCUIApplication) {
        XCTAssertTrue(app.descendants(matching: .any).matching(identifier: "promptField").firstMatch.waitForExistence(timeout: 15))
        openPage("Files", app: app)
        let file = app.staticTexts["hello.txt"].firstMatch
        XCTAssertTrue(file.waitForExistence(timeout: 10))
        file.tap()
        XCTAssertTrue(app.staticTexts["fileContents"].waitForExistence(timeout: 5))
    }

    func testNativeGitInitDiffStageCommitAndRestart() {
        let app = XCUIApplication()
        app.launchEnvironment["CODEX_UI_TEST_PROJECT"] = "1"
        app.launchEnvironment["CODEX_UI_TEST_GIT"] = "1"
        app.launchEnvironment["CODEX_UI_TEST_RESET"] = "1"
        app.launch()
        XCTAssertTrue(app.descendants(matching: .any).matching(identifier: "promptField").firstMatch.waitForExistence(timeout: 15))
        openPage("Activity", app: app)
        app.buttons["Git"].tap()
        app.buttons["gitInitialize"].tap()
        let file = app.descendants(matching: .any).matching(identifier: "gitFile_hello.txt").firstMatch
        XCTAssertTrue(file.waitForExistence(timeout: 10))
        file.tap()
        let diff = app.staticTexts["gitDiffText"]
        XCTAssertTrue(diff.waitForExistence(timeout: 5))
        XCTAssertTrue(diff.label.contains("+Hello from the project."))
        attach("Native Git diff", app: app)
        app.buttons["gitStageFile"].tap()
        for (identifier, value) in [("gitAuthorName", "Native Tester"), ("gitAuthorEmail", "native@example.com"), ("gitCommitMessage", "Initial native commit")] {
            let field = app.descendants(matching: .any).matching(identifier: identifier).firstMatch
            XCTAssertTrue(field.waitForExistence(timeout: 5))
            if !field.isHittable { app.swipeUp() }
            field.tap()
            field.typeText(value)
            let done = app.toolbars.buttons["Done"]
            if done.exists { done.tap() }
        }
        app.swipeUp()
        let review = app.buttons["gitReviewCommit"]
        XCTAssertTrue(review.waitForExistence(timeout: 5))
        review.tap()
        XCTAssertTrue(app.buttons["Create commit"].waitForExistence(timeout: 5))
        attach("Review staged Git commit", app: app)
        app.buttons["Create commit"].tap()
        XCTAssertTrue(app.staticTexts["Working tree is clean"].waitForExistence(timeout: 10))
        app.terminate()
        app.launchEnvironment.removeValue(forKey: "CODEX_UI_TEST_RESET")
        app.launch()
        XCTAssertTrue(app.descendants(matching: .any).matching(identifier: "promptField").firstMatch.waitForExistence(timeout: 15))
        openPage("Activity", app: app)
        app.buttons["Git"].tap()
        app.buttons["Refresh Git status"].tap()
        XCTAssertTrue(app.staticTexts["Working tree is clean"].waitForExistence(timeout: 10))
        attach("Native Git survives restart", app: app)
    }

    private func openPage(_ page: String, app: XCUIApplication) {
        let tab = app.tabBars.buttons[page]
        let button = tab.exists ? tab : app.buttons[page].firstMatch
        XCTAssertTrue(button.waitForExistence(timeout: 5))
        button.tap()
        if page != "Task" { XCTAssertTrue(app.navigationBars[page].waitForExistence(timeout: 5)) }
    }

    private func attach(_ name: String, app: XCUIApplication) {
        let attachment = XCTAttachment(screenshot: app.screenshot())
        attachment.name = name
        attachment.lifetime = .keepAlways
        add(attachment)
    }
}
