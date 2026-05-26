package service

import (
	"context"
	"errors"
	"time"

	"github.com/phaedrus/curb/internal/platform"
)

var (
	ErrNotificationsDisabled    = errors.New("local notifications are disabled")
	ErrNotificationsUnavailable = errors.New("local notifications are unavailable")
)

type NotificationView struct {
	Enabled    bool   `json:"enabled"`
	Available  bool   `json:"available"`
	Status     string `json:"status"`
	Message    string `json:"message"`
	LastTestAt string `json:"last_test_at,omitempty"`
	LastError  string `json:"last_error,omitempty"`
}

func (s *Service) NotificationHealth(context.Context) (NotificationView, error) {
	cfg := s.currentConfig()
	view := newNotificationView(cfg.Alerts.LocalNotifications, s.notificationCapability())
	s.notificationMu.Lock()
	defer s.notificationMu.Unlock()
	return mergeNotificationLast(view, s.notification), nil
}

func (s *Service) TestNotification(context.Context) (NotificationView, error) {
	cfg := s.currentConfig()
	view := newNotificationView(cfg.Alerts.LocalNotifications, s.notificationCapability())
	if !view.Enabled {
		s.recordNotification(view)
		return view, ErrNotificationsDisabled
	}
	if !view.Available {
		s.recordNotification(view)
		return view, ErrNotificationsUnavailable
	}
	if err := s.notify("Curb notification test", "Curb can deliver local agent alerts."); err != nil {
		view.Status = "error"
		view.Message = err.Error()
		view.Available = false
		view.LastError = err.Error()
		view.LastTestAt = time.Now().UTC().Format(time.RFC3339)
		s.recordNotification(view)
		return view, err
	}
	view.Status = "delivered"
	view.Message = "test notification delivered"
	view.LastTestAt = time.Now().UTC().Format(time.RFC3339)
	s.recordNotification(view)
	return view, nil
}

func (s *Service) notificationCapability() platform.NotificationCapability {
	if s.notifyCaps == nil {
		return platform.NotificationCapabilityStatus()
	}
	return s.notifyCaps()
}

func (s *Service) recordNotification(view NotificationView) {
	s.notificationMu.Lock()
	defer s.notificationMu.Unlock()
	s.notification = view
}

func newNotificationView(enabled bool, capability platform.NotificationCapability) NotificationView {
	status := capability.Status
	if status == "available" {
		status = "ready"
	}
	view := NotificationView{
		Enabled:   enabled,
		Available: enabled && capability.Supported,
		Status:    status,
		Message:   capability.Message,
	}
	if !enabled {
		view.Status = "disabled"
		view.Message = "local notifications are disabled in Curb policy"
		view.Available = false
	} else if !capability.Supported {
		view.Status = "unavailable"
	}
	return view
}

func mergeNotificationLast(current NotificationView, last NotificationView) NotificationView {
	if last.LastTestAt != "" {
		current.LastTestAt = last.LastTestAt
	}
	if last.LastError != "" {
		current.LastError = last.LastError
	}
	if current.Enabled && current.Available && (last.Status == "delivered" || last.Status == "error") {
		current.Status = last.Status
		current.Message = last.Message
		current.Available = last.Available
	}
	return current
}
