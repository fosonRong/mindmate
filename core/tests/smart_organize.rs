//! smart_organize 单元测试（v1.4.1）

#[cfg(test)]
mod tests {
    use mindmate_core::smart_organize::{follow_up_todo, suggest_action_items, suggest_kind};
    use chrono::NaiveDate;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    #[test]
    fn 分类_链接判参考() {
        let s = suggest_kind("看看这篇 https://example.com/a 讲排期的");
        assert_eq!(s.kind, "reference");
        assert!(s.confidence >= 0.9);
    }

    #[test]
    fn 分类_日期时间词判待办() {
        assert_eq!(suggest_kind("明天下午3点开会").kind, "todo");
        assert_eq!(suggest_kind("周三早上8点半跑步").kind, "todo");
        assert_eq!(suggest_kind("10月1日看升旗").kind, "todo");
    }

    #[test]
    fn 分类_动作词短句判待办() {
        assert_eq!(suggest_kind("交房租").kind, "todo");
        assert_eq!(suggest_kind("预约体检").kind, "todo");
    }

    #[test]
    fn 分类_普通想法判记录() {
        assert_eq!(suggest_kind("今天的夕阳真好看").kind, "note");
        assert_eq!(suggest_kind("").kind, "note");
    }

    #[test]
    fn 跟进识别_等回复() {
        let today = d(2026, 9, 26);
        let t = follow_up_todo("等待产品经理的回复，需求细节还没定", today).unwrap();
        assert_eq!(t.title, "跟进：产品经理");
        assert_eq!(t.date, "2026-09-29");
        assert_eq!(t.tag.as_deref(), Some("跟进"));
    }

    #[test]
    fn 跟进识别_催进度() {
        let today = d(2026, 9, 26);
        let t = follow_up_todo("催一下供应商的报价结果", today).unwrap();
        assert_eq!(t.title, "跟进：供应商的报价");
        assert_eq!(t.tag.as_deref(), Some("跟进"));
    }

    #[test]
    fn 跟进识别_无匹配返回空() {
        let today = d(2026, 9, 26);
        assert!(follow_up_todo("今天的夕阳真好看", today).is_none());
    }

    #[test]
    fn 行动项提炼_动作句保留并去日期() {
        let today = d(2026, 9, 26);
        let contents = vec![
            "会上确认了下周一提交季度报告；顺便聊了团建".to_string(),
            "纯闲聊没有行动内容的一段话而已".to_string(),
        ];
        let items = suggest_action_items(&contents, today);
        assert!(!items.is_empty());
        assert!(items.iter().any(|x| x.title.contains("季度报告")), "应提炼出提交报告");
        assert!(items.iter().all(|x| x.tag.as_deref() == Some("行动项")));
        assert!(items.len() <= 8);
    }

    #[test]
    fn 行动项提炼_无日期默认明天() {
        let today = d(2026, 9, 26);
        let contents = vec!["需要确认供应商报价".to_string()];
        let items = suggest_action_items(&contents, today);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].date, "2026-09-27");
    }

    #[test]
    fn 行动项提炼_重复句去重() {
        let today = d(2026, 9, 26);
        let contents = vec!["需要提交报告。需要提交报告。需要提交报告。".to_string()];
        let items = suggest_action_items(&contents, today);
        assert_eq!(items.len(), 1, "重复句去重");
    }
}
